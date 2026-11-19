//! Runs runtime calls with smoldot's executor against a [`View`].
//!
//! smoldot's `runtime_call` is a state machine: every storage read, key
//! lookup or signature check pauses it and is answered right away, so nothing
//! is spawned and no threads are involved.

use smoldot::executor::{
    host::{Config as HostConfig, HeapPages, HostVmPrototype, LogEmitInfo},
    runtime_call::{self, RuntimeCall, StorageProofSizeBehavior},
    storage_diff::TrieDiff,
    vm::ExecHint,
};
use smoldot::trie::{Nibble, TrieEntryVersion, bytes_to_nibbles, nibbles_to_bytes_suffix_extend};

use crate::error::{Error, Result};
use crate::storage::{Diff, View};

/// Heap size when `:heappages` is absent, as in Substrate.
const DEFAULT_HEAP_PAGES: u32 = 2048;

/// How signature checks requested by the runtime are answered.
#[derive(Clone, Debug, Default)]
pub struct SignaturePolicy {
    /// Keys whose signatures are accepted even when invalid. The fork puts the
    /// relay chain's BABE authorities here: the relay headers it forges keep
    /// their original, now stale, seals.
    pub trusted_keys: Vec<[u8; 32]>,
    /// Accept chopsticks-style "magic" signatures (`0xdeadbeef` followed by
    /// `0xcd` bytes) so any account can be impersonated. Off by default, so
    /// that signing bugs still surface.
    pub magic: bool,
}

fn is_magic_signature(sig: &[u8]) -> bool {
    sig.len() > 4
        && sig.starts_with(&[0xde, 0xad, 0xbe, 0xef])
        && sig[4..].iter().all(|b| *b == 0xcd)
}

/// Runtime identity, as reported by `Core_version`.
#[derive(Clone, Debug)]
pub struct RuntimeInfo {
    pub spec_name: String,
    pub impl_name: String,
    pub spec_version: u32,
    pub impl_version: u32,
    pub transaction_version: Option<u32>,
    pub apis: Vec<([u8; 8], u32)>,
}

pub struct CallOutput {
    pub output: Vec<u8>,
    pub changes: Diff,
}

/// A compiled runtime, reused across calls.
pub struct Runtime {
    /// Taken while a call runs, and always put back.
    proto: Option<HostVmPrototype>,
    pub info: RuntimeInfo,
}

impl Runtime {
    /// Compile the `:code` found in `state`.
    pub fn from_view(state: &View<'_>) -> Result<Self> {
        let code = state
            .get(None, b":code")
            .ok_or_else(|| Error::Runtime("no :code in state".into()))?;
        let heap_pages = state
            .get(None, b":heappages")
            .and_then(|b| codec::Decode::decode(&mut &b[..]).ok())
            .map(|pages: u64| pages as u32)
            .unwrap_or(DEFAULT_HEAP_PAGES);
        let proto = HostVmPrototype::new(HostConfig {
            module: code,
            heap_pages: HeapPages::from(heap_pages),
            // Lazy validation compiles functions on first use, so the runtime
            // starts in milliseconds; wasmtime would compile it all upfront.
            exec_hint: ExecHint::ForceWasmi {
                lazy_validation: true,
            },
            allow_unresolved_imports: false,
        })
        .map_err(|e| Error::Runtime(format!("instantiation: {e}")))?;
        let v = proto.runtime_version().decode();
        let info = RuntimeInfo {
            spec_name: v.spec_name.into(),
            impl_name: v.impl_name.into(),
            spec_version: v.spec_version,
            impl_version: v.impl_version,
            transaction_version: v.transaction_version,
            apis: v.apis.map(|a| (a.name_hash, a.version)).collect(),
        };
        Ok(Runtime {
            proto: Some(proto),
            info,
        })
    }

    /// Run `function` against `state`. Changes are returned, never applied.
    pub fn call(
        &mut self,
        state: &View<'_>,
        policy: &SignaturePolicy,
        function: &str,
        param: &[u8],
    ) -> Result<CallOutput> {
        let vm = self
            .proto
            .take()
            .expect("the prototype is put back after every call");
        let started = runtime_call::run(runtime_call::Config {
            virtual_machine: vm,
            function_to_call: function,
            parameter: core::iter::once(param),
            storage_main_trie_changes: TrieDiff::empty(),
            // Block authoring asks for the proof size; any constant will do as
            // nothing here produces proofs.
            storage_proof_size_behavior: StorageProofSizeBehavior::ConstantReturnValue(0),
            max_log_level: 3,
            calculate_trie_changes: false,
        });
        let (proto, result) = match started {
            Ok(call) => drive(call, state, policy),
            Err((e, proto)) => (proto, Err(e.to_string())),
        };
        self.proto = Some(proto);
        result.map_err(|e| Error::Runtime(format!("{function}: {e}")))
    }
}

/// Answer the executor's requests until the call finishes.
fn drive(
    mut call: RuntimeCall,
    state: &View<'_>,
    policy: &SignaturePolicy,
) -> (HostVmPrototype, core::result::Result<CallOutput, String>) {
    loop {
        call = match call {
            RuntimeCall::Finished(Ok(success)) => {
                let mut changes = Diff::default();
                for (child, key, value) in success.storage_changes.storage_changes_iter_unordered()
                {
                    changes.set(child, key, value);
                }
                let output = success.virtual_machine.value().as_ref().to_vec();
                return (
                    success.virtual_machine.into_prototype(),
                    Ok(CallOutput { output, changes }),
                );
            }
            RuntimeCall::Finished(Err(e)) => return (e.prototype, Err(e.detail.to_string())),
            RuntimeCall::Offchain(ctx) => {
                return (
                    ctx.into_prototype(),
                    Err("offchain worker APIs are not available".into()),
                );
            }
            RuntimeCall::StorageGet(req) => {
                let child = req.child_trie().map(|c| c.as_ref().to_vec());
                let value = state.get(child.as_deref(), req.key().as_ref());
                req.inject_value(value.map(|v| (core::iter::once(v), TrieEntryVersion::V1)))
            }
            // Like chopsticks, only each call's own diff goes into the state
            // root: no merkle values of untouched subtrees and no branch-node
            // lookups, so roots are not authoritative.
            RuntimeCall::ClosestDescendantMerkleValue(req) => req.resume_unknown(),
            RuntimeCall::NextKey(req) if req.branch_nodes() => {
                req.inject_key(None::<core::iter::Empty<Nibble>>)
            }
            RuntimeCall::NextKey(req) => {
                let key = nibbles_to_bytes_suffix_extend(req.key()).collect::<Vec<_>>();
                let prefix: Vec<Nibble> = req.prefix().collect();
                let child = req.child_trie().map(|c| c.as_ref().to_vec());
                // Odd-length keys only come from state root calculation,
                // answered above; the padded key is then an inclusive bound.
                let inclusive = req.or_equal() || req.key().count() % 2 == 1;
                let found = state
                    .next_key(child.as_deref(), &key, inclusive)
                    .filter(|k| {
                        bytes_to_nibbles(k.iter().copied())
                            .take(prefix.len())
                            .eq(prefix.iter().copied())
                    });
                req.inject_key(found.map(|k| bytes_to_nibbles(k.iter().copied())))
            }
            RuntimeCall::SignatureVerification(req) => {
                let trusted = policy
                    .trusted_keys
                    .iter()
                    .any(|k| k[..] == *req.public_key().as_ref());
                if trusted || (policy.magic && is_magic_signature(req.signature().as_ref())) {
                    req.resume_success()
                } else {
                    req.verify_and_resume()
                }
            }
            RuntimeCall::LogEmit(req) => {
                if let LogEmitInfo::Log {
                    log_level,
                    target,
                    message,
                } = req.info()
                {
                    let level = match log_level {
                        1 => log::Level::Error,
                        2 => log::Level::Warn,
                        3 => log::Level::Info,
                        4 => log::Level::Debug,
                        _ => log::Level::Trace,
                    };
                    log::log!(target: "runtime", level, "[{}] {}", target.as_ref(), message.as_ref());
                }
                req.resume()
            }
            RuntimeCall::OffchainStorageSet(req) => req.resume(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_signature_shape() {
        let mut sig = vec![0xde, 0xad, 0xbe, 0xef];
        sig.extend([0xcd; 60]);
        assert!(is_magic_signature(&sig));
        sig[10] = 0;
        assert!(!is_magic_signature(&sig));
        assert!(!is_magic_signature(&[0xde, 0xad, 0xbe, 0xef]));
    }
}
