//! pallet-pass call filters, shared by devices and session keys.

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use sube::scales::TypeDef;
use sube::{DynValue, Error, Metadata, PreparedCall, Result};

/// A call identified by its pallet and call indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallIndex {
    pub pallet: u8,
    pub call: u8,
}

/// The runtime's `DeviceFilter`: which calls a device or session key may
/// dispatch.
///
/// The runtime's `Spend` variant takes per-asset limits and is not supported.
/// Sessions may never be `Admin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeviceFilter {
    /// Unrestricted.
    Admin,
    /// Any call of the listed pallets.
    Pallets(Vec<u8>),
    /// Only the listed calls.
    Calls(Vec<CallIndex>),
}

impl DeviceFilter {
    /// Parse `admin`, `pallets:Pallet,...` or `calls:Pallet/call,...`,
    /// resolving the names against `metadata`.
    pub fn parse(metadata: &Metadata, text: &str) -> Result<Self> {
        if text.eq_ignore_ascii_case("admin") {
            return Ok(Self::Admin);
        }
        if let Some(calls) = text.strip_prefix("calls:") {
            return Self::calls(metadata, calls.split(','));
        }
        if let Some(pallets) = text.strip_prefix("pallets:") {
            return Self::pallets(metadata, pallets.split(','));
        }
        if text.starts_with("spend:") {
            return Err(Error::OperationFailed(
                "spend filters are not supported: the runtime's Spend filter takes per-asset limits"
                    .into(),
            ));
        }
        Err(Error::OperationFailed(format!(
            "filter {text:?} must be calls:pallet/call,..., pallets:pallet,..., or admin"
        )))
    }

    /// Allow the named `pallet/call` paths.
    pub fn calls<'a>(
        metadata: &Metadata,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self> {
        let calls = paths
            .into_iter()
            .map(|path| resolve_call(metadata, path.trim()))
            .collect::<Result<Vec<_>>>()?;
        if calls.is_empty() {
            return Err(Error::BadInput);
        }
        Ok(Self::Calls(calls))
    }

    /// Allow every call of the named pallets.
    pub fn pallets<'a>(
        metadata: &Metadata,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self> {
        let pallets = names
            .into_iter()
            .map(|name| {
                let name = name.trim();
                metadata
                    .pallet_by_name(name)
                    .map(|pallet| pallet.index)
                    .ok_or_else(|| Error::PalletNotFound(name.into()))
            })
            .collect::<Result<Vec<_>>>()?;
        if pallets.is_empty() {
            return Err(Error::BadInput);
        }
        Ok(Self::Pallets(pallets))
    }

    /// Allow exactly the call `call` encodes.
    pub fn this_call(call: &PreparedCall) -> Result<Self> {
        match call.bytes.as_slice() {
            [pallet, call, ..] => Ok(Self::Calls(vec![CallIndex {
                pallet: *pallet,
                call: *call,
            }])),
            _ => Err(Error::BadInput),
        }
    }

    /// Whether the SCALE-encoded runtime call passes this filter.
    pub fn allows(&self, encoded_call: &[u8]) -> bool {
        let [pallet, call, ..] = *encoded_call else {
            return false;
        };
        match self {
            Self::Admin => true,
            Self::Pallets(pallets) => pallets.contains(&pallet),
            Self::Calls(calls) => calls.contains(&CallIndex { pallet, call }),
        }
    }

    /// The filter as the runtime type, to be encoded against metadata.
    ///
    /// The runtime stores the sets as `BTreeSet`s, so they are encoded sorted
    /// and deduplicated, exactly as they will be stored.
    pub fn runtime_value(&self) -> DynValue {
        match self {
            Self::Admin => DynValue::obj(&[("Admin", DynValue::Null)]),
            Self::Pallets(pallets) => DynValue::obj(&[(
                "Pallets",
                DynValue::Seq(sorted(pallets).into_iter().map(DynValue::from).collect()),
            )]),
            Self::Calls(calls) => DynValue::obj(&[(
                "Calls",
                DynValue::Seq(
                    sorted(calls)
                        .into_iter()
                        .map(|call| {
                            DynValue::Seq(vec![
                                DynValue::from(call.pallet),
                                DynValue::from(call.call),
                            ])
                        })
                        .collect(),
                ),
            )]),
        }
    }
}

fn sorted<T: Copy + Ord>(items: &[T]) -> Vec<T> {
    let mut items = items.to_vec();
    items.sort_unstable();
    items.dedup();
    items
}

fn resolve_call(metadata: &Metadata, path: &str) -> Result<CallIndex> {
    let (pallet_name, call_name) = path.split_once('/').ok_or(Error::BadInput)?;
    let pallet = metadata
        .pallet_by_name(pallet_name)
        .ok_or_else(|| Error::PalletNotFound(pallet_name.into()))?;
    let calls_ty = pallet.calls_ty.ok_or(Error::CallNotFound)?;
    let Some(TypeDef::Variant(calls)) = metadata.registry.resolve(calls_ty) else {
        return Err(Error::CallNotFound);
    };
    let call = calls
        .variants()
        .find(|variant| variant.name().eq_ignore_ascii_case(call_name))
        .ok_or(Error::CallNotFound)?;
    Ok(CallIndex {
        pallet: pallet.index,
        call: call.index(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn kreivo() -> Metadata {
        Metadata::from_bytes(include_bytes!("../../sube/tests/fixtures/kreivo.scale")).unwrap()
    }

    #[test]
    fn parses_names_against_metadata() {
        let metadata = kreivo();
        let system = metadata.pallet_by_name("System").unwrap().index;
        let balances = metadata.pallet_by_name("Balances").unwrap().index;
        assert_eq!(
            DeviceFilter::parse(&metadata, "admin").unwrap(),
            DeviceFilter::Admin
        );
        assert_eq!(
            DeviceFilter::parse(&metadata, "pallets:Balances, System").unwrap(),
            DeviceFilter::Pallets(vec![balances, system])
        );
        let DeviceFilter::Calls(calls) =
            DeviceFilter::parse(&metadata, "calls:System/remark").unwrap()
        else {
            panic!("expected a call filter");
        };
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].pallet, system);
        assert!(DeviceFilter::parse(&metadata, "calls:System/nope").is_err());
        assert!(DeviceFilter::parse(&metadata, "pallets:Nope").is_err());
        assert!(DeviceFilter::parse(&metadata, "calls:").is_err());
    }

    #[test]
    fn spend_filters_are_refused_with_a_clear_error() {
        let error = DeviceFilter::parse(&kreivo(), "spend:Balances:1:transfer_keep_alive")
            .unwrap_err()
            .to_string();
        assert!(error.contains("spend filters are not supported"), "{error}");
        let error = DeviceFilter::parse(&kreivo(), "everything")
            .unwrap_err()
            .to_string();
        assert!(error.contains("must be calls:"), "{error}");
    }

    #[test]
    fn local_filter_rejects_disallowed_call() {
        let filter = DeviceFilter::Calls(vec![CallIndex { pallet: 5, call: 2 }]);
        assert!(filter.allows(&[5, 2, 9]));
        assert!(!filter.allows(&[5, 3, 9]));
        assert!(!filter.allows(&[5]));
        let pallets = DeviceFilter::Pallets(vec![5]);
        assert!(pallets.allows(&[5, 3]));
        assert!(!pallets.allows(&[6, 3]));
        assert!(DeviceFilter::Admin.allows(&[6, 3]));
    }

    #[test]
    fn this_call_reads_the_call_indices() {
        let call = PreparedCall {
            pallet: "System".into(),
            call: "remark".into(),
            bytes: vec![0, 7, 4, 1],
        };
        assert_eq!(
            DeviceFilter::this_call(&call).unwrap(),
            DeviceFilter::Calls(vec![CallIndex { pallet: 0, call: 7 }])
        );
    }

    #[test]
    fn runtime_value_is_sorted_and_deduplicated() {
        let filter = DeviceFilter::Pallets(vec![10, 5, 10]);
        assert_eq!(
            filter.runtime_value(),
            DynValue::obj(&[(
                "Pallets",
                DynValue::Seq(vec![DynValue::from(5u8), DynValue::from(10u8)])
            )])
        );
    }
}
