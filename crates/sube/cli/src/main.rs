use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Read;
#[cfg(feature = "wallet")]
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

mod profiles;
mod tui;

/// Sube — query and explore Substrate chains
#[derive(Parser)]
#[command(name = "sube", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// URL path to query, e.g. system/account/0x1234
    /// Compatibility shorthand for `sube query PATH`.
    path: Option<String>,

    /// Chain endpoint (wss://kreivo.io by default)
    #[arg(short, long, default_value = "wss://kreivo.io")]
    chain: String,

    /// Watch for changes (re-query on each new finalized block)
    #[arg(short, long)]
    watch: bool,

    /// Override the versioned profile store path.
    #[arg(long, global = true)]
    profiles: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Query storage or runtime constants.
    Query {
        path: String,
        #[arg(short, long)]
        watch: bool,
    },
    /// Validate and encode a call. This does not submit by default.
    Tx {
        /// Pallet/call path, e.g. balances/transfer_keep_alive.
        path: String,
        /// Call body as SCALE text or JSON.
        #[arg(long, conflicts_with_all = ["body_file", "body_stdin"])]
        body: Option<String>,
        /// Read the whole call body from a file.
        #[arg(long, conflicts_with_all = ["body", "body_stdin"])]
        body_file: Option<PathBuf>,
        /// Read the whole call body from stdin.
        #[arg(long, conflicts_with_all = ["body", "body_file"])]
        body_stdin: bool,
        #[arg(long, value_enum, default_value_t = BodyFormat::Text)]
        body_format: BodyFormat,
        /// Emit a JSON artifact instead of only call hex.
        #[arg(long)]
        json: bool,
        /// Explicitly request submission (requires a configured signer).
        #[arg(long)]
        submit: bool,
        /// Skip interactive confirmation when submission is supported.
        #[arg(long, requires = "submit")]
        yes: bool,
        /// Return after best-chain inclusion instead of finalization.
        #[arg(long, requires = "submit")]
        best: bool,
    },
    /// List, create, and select non-secret chain-bound profiles.
    Profile {
        #[command(subcommand)]
        command: Option<ProfileCommand>,
    },
    /// Validate a profile against the chain and make it active.
    Connect {
        profile: String,
        /// Required for pass profiles without an exact reusable session.
        #[arg(long)]
        session_policy: Option<String>,
        /// Request less than the runtime maximum session duration.
        #[arg(long)]
        session_duration: Option<u32>,
        /// Skip confirmation before registering/updating a pass session.
        #[arg(long)]
        yes: bool,
    },
    #[cfg(feature = "pass")]
    /// Enroll and manage pallet-pass identities.
    Pass {
        #[command(subcommand)]
        command: PassCommand,
    },
}

#[derive(Subcommand)]
enum ProfileCommand {
    List,
    AddWallet {
        name: String,
        #[arg(long)]
        genesis: String,
        #[arg(long)]
        account: String,
        /// Read a mnemonic from stdin and write it to the OS secure store.
        #[arg(long)]
        mnemonic_stdin: bool,
    },
    Use {
        name: String,
    },
}

#[cfg(feature = "pass")]
#[derive(Subcommand)]
enum PassCommand {
    Enroll {
        /// Exact 32-byte hashed user id (hex).
        #[arg(long)]
        user_id: String,
        /// Ordinary wallet profile paying for registration.
        #[arg(long)]
        registrar: String,
        /// Substrate-key wallet profile used as the first device.
        #[arg(long)]
        device_wallet: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        submit: bool,
        #[arg(long, requires = "submit")]
        yes: bool,
    },
    Device {
        #[command(subcommand)]
        command: DeviceCommand,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[cfg(feature = "pass")]
#[derive(Subcommand)]
enum DeviceCommand {
    Add {
        profile: String,
        #[arg(long)]
        device_wallet: String,
        /// Explicit filter: calls:pallet/call,... or pallets:pallet,...
        #[arg(long)]
        filter: String,
        #[arg(long)]
        admin_confirm: bool,
        #[arg(long)]
        submit: bool,
        #[arg(long, requires = "submit")]
        yes: bool,
    },
    Remove {
        profile: String,
        #[arg(long)]
        device_id: String,
        #[arg(long)]
        submit: bool,
        #[arg(long, requires = "submit")]
        yes: bool,
    },
}

#[cfg(feature = "pass")]
#[derive(Subcommand)]
enum SessionCommand {
    /// Delete the local session secret; the on-chain entry expires on schedule.
    Forget { profile: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum BodyFormat {
    Text,
    Json,
}

#[derive(Clone, Copy)]
struct TxBehavior {
    json: bool,
    submit: bool,
    yes: bool,
    best: bool,
}

fn main() -> Result<()> {
    smol::block_on(run())
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let profile_path = cli.profiles.unwrap_or_else(profiles::default_path);

    match cli.command {
        Some(Command::Query { path, watch: true }) => watch(&cli.chain, &path).await,
        Some(Command::Query { path, watch: false }) => oneshot(&cli.chain, &path).await,
        Some(Command::Tx {
            path,
            body,
            body_file,
            body_stdin,
            body_format,
            json,
            submit,
            yes,
            best,
        }) => {
            let body = read_body(body, body_file, body_stdin)?;
            tx(
                &cli.chain,
                &profile_path,
                &path,
                &body,
                body_format,
                TxBehavior {
                    json,
                    submit,
                    yes,
                    best,
                },
            )
            .await
        }
        Some(Command::Profile { command }) => profile_command(&profile_path, command),
        Some(Command::Connect {
            profile,
            session_policy,
            session_duration,
            yes,
        }) => {
            connect_profile(
                &cli.chain,
                &profile_path,
                &profile,
                session_policy.as_deref(),
                session_duration,
                yes,
            )
            .await
        }
        #[cfg(feature = "pass")]
        Some(Command::Pass { command }) => pass_command(&cli.chain, &profile_path, command).await,
        None => match cli.path {
            Some(path) if cli.watch => watch(&cli.chain, &path).await,
            Some(path) => oneshot(&cli.chain, &path).await,
            None => tui::run(&cli.chain, &profile_path).await,
        },
    }
}

fn read_body(inline: Option<String>, file: Option<PathBuf>, from_stdin: bool) -> Result<String> {
    match (inline, file, from_stdin) {
        (Some(body), None, false) => Ok(body),
        (None, Some(path), false) => Ok(std::fs::read_to_string(path)?),
        (None, None, true) => {
            let mut body = String::new();
            std::io::stdin().read_to_string(&mut body)?;
            Ok(body)
        }
        _ => anyhow::bail!("provide exactly one of --body, --body-file, or --body-stdin"),
    }
}

async fn tx(
    chain_url: &str,
    profile_path: &std::path::Path,
    path: &str,
    body: &str,
    body_format: BodyFormat,
    behavior: TxBehavior,
) -> Result<()> {
    #[cfg(not(feature = "wallet"))]
    let _ = (profile_path, behavior.yes, behavior.best);
    if behavior.submit {
        #[cfg(feature = "wallet")]
        {
            let profiles = profiles::Profiles::load(profile_path)?;
            let can_sign = match profiles.active() {
                Some(profiles::Profile::Wallet(_)) => true,
                #[cfg(feature = "pass")]
                Some(profiles::Profile::Pass(profile)) => profile.session.is_some(),
                _ => false,
            };
            if !can_sign {
                anyhow::bail!(
                    "submission requires an active signing profile; no transaction was submitted"
                );
            }
        }
        #[cfg(not(feature = "wallet"))]
        anyhow::bail!("this build has no signing support; no transaction was submitted");
    }

    eprintln!("Connecting to {chain_url}...");
    #[allow(unused_mut)]
    let mut chain = sube::Sube::connect(chain_url).await?;
    let prepared = match body_format {
        BodyFormat::Text => chain.prepare_call(path, &sube::Text(body))?,
        BodyFormat::Json => {
            let value: serde_json::Value = serde_json::from_str(body)?;
            chain.prepare_call(path, &value)?
        }
    };

    #[cfg(feature = "wallet")]
    {
        let profiles = profiles::Profiles::load(profile_path)?;
        match profiles.active() {
            Some(profiles::Profile::Wallet(profile)) => {
                return tx_with_wallet(&mut chain, chain_url, &prepared, profile, behavior).await;
            }
            #[cfg(feature = "pass")]
            Some(profiles::Profile::Pass(profile)) => {
                return tx_with_pass_session(&mut chain, chain_url, &prepared, profile, behavior)
                    .await;
            }
            _ => {}
        }
    }

    if behavior.submit {
        anyhow::bail!("submission requires an active wallet profile; no transaction was submitted");
    }
    print_call_artifact(chain_url, &prepared, behavior.json);
    Ok(())
}

fn print_call_artifact(chain_url: &str, prepared: &sube::PreparedCall, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "version": 1,
                "chain": chain_url,
                "pallet": prepared.pallet,
                "call": prepared.call,
                "callHex": prepared.hex,
                "submitted": false,
            })
        );
    } else {
        println!("{}", prepared.hex);
    }
}

#[cfg(feature = "wallet")]
async fn wallet_signer(
    profile: &profiles::WalletProfile,
) -> Result<sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>> {
    use libwallet::vault::Vault;

    if !profile.scheme.eq_ignore_ascii_case("sr25519") {
        anyhow::bail!("unsupported wallet profile scheme {}", profile.scheme);
    }
    let keys = libwallet::vault::OSKeyring::<()>::new(&profile.secure_entry, None);
    let mut vault = libwallet::Substrate::new(keys);
    let signer = vault.unlock(None, ()).await?;
    let public = signer.public();
    if public.as_ref() != profile.account {
        anyhow::bail!("wallet secure entry does not match the profile account");
    }
    Ok(sube::libwallet::LibwalletSigner::new(
        signer,
        profile.account,
        sube::libwallet::SignatureScheme::Sr25519,
    ))
}

#[cfg(feature = "wallet")]
async fn tx_with_wallet(
    chain: &mut sube::Sube,
    chain_url: &str,
    call: &sube::PreparedCall,
    profile: &profiles::WalletProfile,
    behavior: TxBehavior,
) -> Result<()> {
    let genesis_hash = chain_genesis(chain).await?;
    if profile.genesis_hash != genesis_hash {
        anyhow::bail!("active profile genesis hash does not match the connected chain");
    }
    let signer = wallet_signer(profile).await?;
    tx_with_assembler(
        chain,
        chain_url,
        call,
        profile.name.as_str(),
        &signer,
        behavior,
    )
    .await
}

#[cfg(feature = "pass")]
async fn tx_with_pass_session(
    chain: &mut sube::Sube,
    chain_url: &str,
    call: &sube::PreparedCall,
    profile: &profiles::PassProfile,
    behavior: TxBehavior,
) -> Result<()> {
    let genesis_hash = chain_genesis(chain).await?;
    if profile.genesis_hash != genesis_hash {
        anyhow::bail!("active profile genesis hash does not match the connected chain");
    }
    let session = profile
        .session
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("active pass profile has no finalized local session"))?;
    let policy = parse_session_policy(chain.metadata(), &session.policy)?;
    let signer = load_session_signer(&session.secure_entry, Some(session.account)).await?;
    let authorizer =
        pass::SessionAuthorizer::new(pass::Account(profile.pass_account), signer, policy);
    tx_with_assembler(
        chain,
        chain_url,
        call,
        profile.name.as_str(),
        &authorizer,
        behavior,
    )
    .await
}

#[cfg(feature = "wallet")]
async fn tx_with_assembler(
    chain: &mut sube::Sube,
    chain_url: &str,
    call: &sube::PreparedCall,
    authorizer_name: &str,
    authorizer: &(impl sube::ExtrinsicAssembler + ?Sized),
    behavior: TxBehavior,
) -> Result<()> {
    let extrinsic = chain
        .build_transaction(call, authorizer, sube::TransactionOptions::default())
        .await?;
    let report = chain.inspect_transaction(&extrinsic).await?;
    if matches!(report.validity, Some(sube::TransactionValidity::Invalid(_))) {
        anyhow::bail!("transaction validation reports known-invalid; submission blocked");
    }

    if behavior.json {
        println!(
            "{}",
            transaction_artifact(chain_url, authorizer_name, &extrinsic, &report)
        );
    } else {
        print_review(chain_url, authorizer_name, &extrinsic, &report);
    }

    if !behavior.submit {
        return Ok(());
    }
    if !behavior.yes && !confirm_submission()? {
        anyhow::bail!("submission canceled; no transaction was submitted");
    }
    let receipt = chain
        .submit_transaction(
            &extrinsic,
            if behavior.best {
                sube::WaitFor::BestBlock
            } else {
                sube::WaitFor::Finalized
            },
        )
        .await?;
    println!(
        "{}",
        serde_json::json!({
            "bestBlockHash": receipt.best_block_hash,
            "finalizedBlockHash": receipt.finalized_block_hash,
            "extrinsicIndex": receipt.extrinsic_index,
            "dispatchOutcome": format!("{:?}", receipt.dispatch_outcome),
            "events": receipt.events.iter().map(|event| serde_json::json!({
                "pallet": event.pallet,
                "variant": event.variant,
                "decoded": event.decoded,
            })).collect::<Vec<_>>(),
        })
    );
    Ok(())
}

#[cfg(feature = "wallet")]
fn transaction_artifact(
    chain_url: &str,
    authorizer: &str,
    extrinsic: &sube::EncodedExtrinsic,
    report: &sube::TransactionReport,
) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "chain": chain_url,
        "genesisHash": format!("0x{}", hex::encode(extrinsic.genesis_hash)),
        "specVersion": extrinsic.spec_version,
        "transactionVersion": extrinsic.transaction_version,
        "checkpoint": {
            "number": extrinsic.checkpoint_number,
            "hash": format!("0x{}", hex::encode(extrinsic.checkpoint_hash)),
            "expiresAt": extrinsic.expires_at,
        },
        "authorizer": authorizer,
        "signingAccount": format!("0x{}", hex::encode(&extrinsic.authorization.signing_account)),
        "nonceAccount": format!("0x{}", hex::encode(&extrinsic.authorization.nonce_account)),
        "nonce": extrinsic.nonce,
        "call": {
            "pallet": extrinsic.call.pallet,
            "name": extrinsic.call.call,
            "hex": extrinsic.call.hex,
        },
        "extrinsicHex": extrinsic.hex,
        "fee": report.partial_fee.map(|fee| fee.to_string()),
        "weight": report.weight.as_ref().map(|weight| serde_json::json!({
            "refTime": weight.ref_time,
            "proofSize": weight.proof_size,
        })),
        "validity": report.validity.as_ref().map(|validity| format!("{validity:?}")),
        "warnings": report.warnings,
        "submitted": false,
    })
}

#[cfg(feature = "wallet")]
fn print_review(
    chain_url: &str,
    authorizer: &str,
    extrinsic: &sube::EncodedExtrinsic,
    report: &sube::TransactionReport,
) {
    println!("Chain: {chain_url}");
    println!("Genesis: 0x{}", hex::encode(extrinsic.genesis_hash));
    println!("Authorizer: {authorizer}");
    println!(
        "Signing account: 0x{}",
        hex::encode(&extrinsic.authorization.signing_account)
    );
    println!(
        "Nonce account: 0x{} (nonce {})",
        hex::encode(&extrinsic.authorization.nonce_account),
        extrinsic.nonce
    );
    println!("Call: {}::{}", extrinsic.call.pallet, extrinsic.call.call);
    println!(
        "Checkpoint: #{} 0x{}",
        extrinsic.checkpoint_number,
        hex::encode(extrinsic.checkpoint_hash)
    );
    println!("Expires: {:?}", extrinsic.expires_at);
    println!("Fee: {:?}", report.partial_fee);
    println!("Weight: {:?}", report.weight);
    println!("Validity: {:?}", report.validity);
    println!("Call hex: {}", extrinsic.call.hex);
    println!("Extrinsic hex: {}", extrinsic.hex);
    for warning in &report.warnings {
        println!("Warning: {warning}");
    }
}

#[cfg(feature = "wallet")]
fn confirm_submission() -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        anyhow::bail!("interactive confirmation requires a terminal; use --yes");
    }
    eprint!("Submit this transaction? [y/N] ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

async fn chain_genesis(chain: &mut sube::Sube) -> Result<[u8; 32]> {
    use sube::Backend;
    Ok(chain.backend().block_info(Some(0)).await?.hash)
}

fn profile_command(path: &std::path::Path, command: Option<ProfileCommand>) -> Result<()> {
    let mut store = profiles::Profiles::load(path)?;
    match command.unwrap_or(ProfileCommand::List) {
        ProfileCommand::List => {
            for profile in &store.profiles {
                let active = if store.active.as_deref() == Some(profile.name()) {
                    "*"
                } else {
                    " "
                };
                println!(
                    "{active} {} 0x{}",
                    profile.name(),
                    hex::encode(profile.genesis_hash())
                );
            }
            for pending in &store.pending_enrollments {
                println!(
                    "~ {} pending enrollment through #{} 0x{}",
                    pending.name,
                    pending.valid_through,
                    hex::encode(pending.genesis_hash)
                );
            }
            Ok(())
        }
        ProfileCommand::AddWallet {
            name,
            genesis,
            account,
            mnemonic_stdin,
        } => {
            let genesis_hash = profiles::parse_hash(&genesis, "genesis hash")?;
            let account = profiles::parse_hash(&account, "account")?;
            let secure_entry = format!("wallet-profile-v1-{name}");
            #[cfg(feature = "wallet")]
            if mnemonic_stdin {
                let mut mnemonic = String::new();
                std::io::stdin().read_to_string(&mut mnemonic)?;
                let keyring = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
                keyring.update(mnemonic.trim())?;
            }
            #[cfg(not(feature = "wallet"))]
            if mnemonic_stdin {
                anyhow::bail!("this build has no wallet secure-store support");
            }
            store.upsert(profiles::Profile::Wallet(profiles::WalletProfile {
                name,
                genesis_hash,
                account,
                secure_entry,
                scheme: "sr25519".into(),
            }));
            store.save(path)
        }
        ProfileCommand::Use { name } => {
            if store.find(&name).is_none() {
                anyhow::bail!("profile {name:?} not found");
            }
            store.active = Some(name);
            store.save(path)
        }
    }
}

async fn connect_profile(
    chain_url: &str,
    profile_path: &std::path::Path,
    name: &str,
    session_policy: Option<&str>,
    session_duration: Option<u32>,
    yes: bool,
) -> Result<()> {
    let mut chain = sube::Sube::connect(chain_url).await?;
    let genesis_hash = chain_genesis(&mut chain).await?;
    let mut store = profiles::Profiles::load(profile_path)?;
    let selected = store
        .find(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("profile {name:?} not found"))?;
    if selected.genesis_hash() != genesis_hash {
        anyhow::bail!("profile genesis hash does not match the connected chain");
    }
    #[cfg(feature = "wallet")]
    if let profiles::Profile::Wallet(profile) = &selected {
        let _ = wallet_signer(profile).await?;
    }
    #[cfg(feature = "pass")]
    if let profiles::Profile::Pass(profile) = &selected {
        connect_pass_session(
            &mut chain,
            chain_url,
            &mut store,
            profile.clone(),
            session_policy,
            session_duration,
            yes,
        )
        .await?;
    }
    #[cfg(not(feature = "pass"))]
    let _ = (session_policy, session_duration, yes);
    store.activate(name, genesis_hash)?;
    store.save(profile_path)?;
    println!(
        "Connected profile {name} to 0x{}",
        hex::encode(genesis_hash)
    );
    Ok(())
}

#[cfg(feature = "pass")]
async fn connect_pass_session(
    chain: &mut sube::Sube,
    chain_url: &str,
    store: &mut profiles::Profiles,
    mut profile: profiles::PassProfile,
    requested_policy: Option<&str>,
    requested_duration: Option<u32>,
    yes: bool,
) -> Result<()> {
    use libwallet::MutableKeyStore;
    use sube::Backend;

    let policy_text = requested_policy
        .map(str::to_owned)
        .or_else(|| {
            profile
                .session
                .as_ref()
                .map(|session| session.policy.clone())
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "pass connect requires --session-policy calls:..., pallets:..., or spend:..."
            )
        })?;
    let policy = parse_session_policy(chain.metadata(), &policy_text)?;
    let checkpoint = chain.backend().block_info(None).await?;

    if let Some(session) = &profile.session
        && session.policy == policy_text
        && session.expires_at > checkpoint.number
        && on_chain_session_matches(chain, &profile, session, &policy).await
    {
        return Ok(());
    }

    if !yes && !confirm_submission()? {
        anyhow::bail!("session registration canceled; profile was not activated");
    }

    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let duration = pass::session::session_duration(&config, requested_duration)?;
    let secure_entry =
        libwallet::pass_session_entry_id(&profile.genesis_hash, &profile.pass_account);

    let existing_account = profile.session.as_ref().map(|session| session.account);
    let (session_signer, pending_seed) = if let Some(expected) = existing_account {
        match load_session_signer(&secure_entry, Some(expected)).await {
            Ok(signer) => (signer, None),
            Err(error) => {
                eprintln!(
                    "Stored session key is unavailable or invalid ({error}); preparing a replacement"
                );
                let (signer, seed) = generate_session_signer().await?;
                (signer, Some(seed))
            }
        }
    } else {
        let (signer, seed) = generate_session_signer().await?;
        (signer, Some(seed))
    };
    let session_account = sube::Signer::account(&session_signer);
    let call = pass::session::prepare_add_session_key(
        chain.metadata(),
        &config,
        session_account,
        &policy,
        Some(duration),
    )?;

    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;
    let device_wallet =
        wallet_profile(store, &profile.device_wallet, profile.genesis_hash)?.clone();
    let device_signer = wallet_signer(&device_wallet).await?;
    let device_public = device_signer.inner().public();
    let device = pass::wallet::WalletDevice::new(
        device_signer.inner(),
        device_public.as_ref(),
        pass::wallet::SignatureType::Sr25519,
    );
    let provider = pass::PassAuthorizer::new(
        &device,
        pass::HashedUserId(profile.user_id),
        config.authority_id,
        context,
        checkpoint.hash,
    )
    .with_challenger(config.challenger);
    let authorizer = pass::PassAuthenticator::new(
        pass::Account(profile.pass_account),
        pass::DeviceId(profile.device_id),
        provider,
    );
    let receipt = review_pass_transaction(
        chain,
        chain_url,
        &profile.name,
        &call,
        &authorizer,
        true,
        true,
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("session registration returned no receipt"))?;
    if receipt.finalized_block_hash.is_none()
        || !matches!(receipt.dispatch_outcome, sube::DispatchOutcome::Success)
    {
        anyhow::bail!("session registration did not finalize successfully; profile unchanged");
    }

    if let Some(mut seed) = pending_seed {
        let mut keys = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
        let result = keys.upsert(&seed);
        seed.fill(0);
        result?;
    }

    profile.session = Some(profiles::SessionProfile {
        account: session_account,
        secure_entry,
        policy: policy_text,
        expires_at: checkpoint.number.saturating_add(u64::from(duration)),
    });
    store.upsert(profiles::Profile::Pass(profile));
    Ok(())
}

#[cfg(feature = "pass")]
async fn generate_session_signer() -> Result<(
    sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>,
    [u8; 32],
)> {
    use libwallet::vault::Vault;

    let mut keys = libwallet::vault::Simple::<(), 32>::generate(&mut rand_core::OsRng);
    let seed = *keys.unlock()?;
    let mut vault = libwallet::Substrate::new(keys);
    let derived = vault.unlock(None, ()).await?;
    let public = derived.public();
    let account: [u8; 32] = public
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("session signer is not AccountId32"))?;
    Ok((
        sube::libwallet::LibwalletSigner::new(
            derived,
            account,
            sube::libwallet::SignatureScheme::Sr25519,
        ),
        seed,
    ))
}

#[cfg(feature = "pass")]
async fn load_session_signer(
    secure_entry: &str,
    expected: Option<[u8; 32]>,
) -> Result<sube::libwallet::LibwalletSigner<libwallet::vault::utils::DerivedSigner>> {
    use libwallet::vault::Vault;

    let keys = libwallet::vault::OSKeyring::<()>::new(secure_entry, None);
    let mut vault = libwallet::Substrate::new(keys);
    let derived = vault.unlock(None, ()).await?;
    let public = derived.public();
    let account: [u8; 32] = public
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("session signer is not AccountId32"))?;
    if expected.is_some_and(|expected| expected != account) {
        anyhow::bail!("stored session key does not match the pass profile");
    }
    Ok(sube::libwallet::LibwalletSigner::new(
        derived,
        account,
        sube::libwallet::SignatureScheme::Sr25519,
    ))
}

#[cfg(feature = "pass")]
async fn on_chain_session_matches(
    chain: &mut sube::Sube,
    profile: &profiles::PassProfile,
    session: &profiles::SessionProfile,
    policy: &pass::SessionPolicy,
) -> bool {
    let candidates: Vec<(String, usize)> = chain
        .metadata()
        .pallet_by_name("Pass")
        .and_then(|pallet| pallet.storage.as_ref().map(|storage| (pallet, storage)))
        .map(|(pallet, storage)| {
            storage
                .entries
                .iter()
                .filter(|entry| entry.name.to_ascii_lowercase().contains("session"))
                .map(|entry| {
                    let keys = match &entry.ty {
                        sube::metadata::StorageEntryType::Plain(_) => 0,
                        sube::metadata::StorageEntryType::Map { hashers, key, .. } => {
                            match chain.metadata().registry.resolve(*key) {
                                Some(
                                    sube::scales::TypeDef::Tuple(keys)
                                    | sube::scales::TypeDef::StructTuple(keys),
                                ) => keys.len(),
                                _ => hashers.len(),
                            }
                        }
                    };
                    (format!("{}/{}", pallet.name, entry.name), keys)
                })
                .collect()
        })
        .unwrap_or_default();
    let pass_hex = format!("0x{}", hex::encode(profile.pass_account));
    let session_hex = format!("0x{}", hex::encode(session.account));
    for (base, key_count) in candidates {
        let path = match key_count {
            0 => base,
            1 => format!("{base}/{session_hex}"),
            _ => format!("{base}/{pass_hex}/{session_hex}"),
        };
        let Ok(response) = chain.query(&path).await else {
            continue;
        };
        if response.is_none() {
            continue;
        }
        let Ok(Some(text)) = response.to_text() else {
            continue;
        };
        if policy_text_matches(&text, policy) {
            return true;
        }
    }
    false
}

#[cfg(feature = "pass")]
fn policy_text_matches(text: &str, policy: &pass::SessionPolicy) -> bool {
    match policy {
        pass::SessionPolicy::Calls(calls) => policy_numbers(text, "Calls").is_some_and(|actual| {
            actual
                == calls
                    .iter()
                    .flat_map(|call| [u128::from(call.pallet), u128::from(call.call)])
                    .collect::<Vec<_>>()
        }),
        pass::SessionPolicy::Pallets(pallets) => {
            policy_numbers(text, "Pallets").is_some_and(|actual| {
                actual
                    == pallets
                        .iter()
                        .map(|pallet| u128::from(*pallet))
                        .collect::<Vec<_>>()
            })
        }
        pass::SessionPolicy::Spend { calls, limit } => {
            let mut calls_then_limit = calls
                .iter()
                .flat_map(|call| [u128::from(call.pallet), u128::from(call.call)])
                .collect::<Vec<_>>();
            calls_then_limit.push(*limit);
            let mut limit_then_calls = vec![*limit];
            limit_then_calls.extend(
                calls
                    .iter()
                    .flat_map(|call| [u128::from(call.pallet), u128::from(call.call)]),
            );
            policy_numbers(text, "Spend")
                .is_some_and(|actual| actual == calls_then_limit || actual == limit_then_calls)
        }
    }
}

#[cfg(feature = "pass")]
fn policy_numbers(text: &str, variant: &str) -> Option<Vec<u128>> {
    let start = text.find(variant)? + variant.len();
    let tail = text.get(start..)?;
    let opening = tail.find(['(', '[', '{'])?;
    let segment = tail.get(opening..)?;
    let mut depth = 0usize;
    let mut end = None;
    for (index, ch) in segment.char_indices() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    end = Some(index);
                    break;
                }
            }
            _ => {}
        }
    }
    let segment = segment.get(..=end?)?;
    let mut numbers = Vec::new();
    let mut current = String::new();
    for ch in segment.chars() {
        if ch.is_ascii_digit() {
            current.push(ch);
        } else if !current.is_empty() {
            numbers.push(current.parse().ok()?);
            current.clear();
        }
    }
    if !current.is_empty() {
        numbers.push(current.parse().ok()?);
    }
    Some(numbers)
}

#[cfg(feature = "pass")]
async fn pass_command(
    chain_url: &str,
    profile_path: &std::path::Path,
    command: PassCommand,
) -> Result<()> {
    use pass::DeviceAuthenticator;
    use sube::Backend;

    if let PassCommand::Session {
        command: SessionCommand::Forget { profile },
    } = &command
    {
        use libwallet::MutableKeyStore;

        let mut store = profiles::Profiles::load(profile_path)?;
        let pass_profile = match store.find(profile).cloned() {
            Some(profiles::Profile::Pass(profile)) => profile,
            _ => anyhow::bail!("pass profile {profile:?} not found"),
        };
        let secure_entry = pass_profile
            .session
            .as_ref()
            .map(|session| session.secure_entry.clone())
            .unwrap_or_else(|| {
                libwallet::pass_session_entry_id(
                    &pass_profile.genesis_hash,
                    &pass_profile.pass_account,
                )
            });
        let mut keys = libwallet::vault::OSKeyring::<()>::new(&secure_entry, None);
        keys.delete()?;
        let mut updated = pass_profile;
        updated.session = None;
        store.upsert(profiles::Profile::Pass(updated));
        store.save(profile_path)?;
        println!("{}", pass::session::FORGET_SESSION_WARNING);
        return Ok(());
    }

    let mut chain = sube::Sube::connect(chain_url).await?;
    let genesis_hash = chain_genesis(&mut chain).await?;
    let mut store = profiles::Profiles::load(profile_path)?;
    let config = pass::PassRuntimeConfig::discover(chain.metadata())?;
    let checkpoint = chain.backend().block_info(None).await?;
    let context = u32::try_from(checkpoint.number)
        .map_err(|_| anyhow::anyhow!("pass context exceeds u32"))?;

    match command {
        PassCommand::Enroll {
            user_id,
            registrar,
            device_wallet,
            name,
            submit,
            yes,
        } => {
            let user_id =
                pass::HashedUserId::from_exact(&hex::decode(user_id.trim_start_matches("0x"))?)?;
            if store.find(&name).is_some() {
                anyhow::bail!("profile {name:?} already exists");
            }
            let registrar_profile = wallet_profile(&store, &registrar, genesis_hash)?.clone();
            let pending = store
                .pending_enrollments
                .iter()
                .find(|pending| {
                    pending.name == name
                        && pending.genesis_hash == genesis_hash
                        && pending.registrar == registrar
                        && pending.device_wallet == device_wallet
                        && pending.user_id == user_id.0
                        && pending.valid_through >= checkpoint.number
                })
                .cloned();
            let pending = if let Some(pending) = pending {
                println!(
                    "Reusing pending enrollment prepared at checkpoint #{}",
                    pending.checkpoint_number
                );
                pending
            } else {
                let device_profile = wallet_profile(&store, &device_wallet, genesis_hash)?.clone();
                let device_signer = wallet_signer(&device_profile).await?;
                let public = device_signer.inner().public();
                let device = pass::wallet::WalletDevice::new(
                    device_signer.inner(),
                    public.as_ref(),
                    pass::wallet::SignatureType::Sr25519,
                );
                let draft = pass::workflow::prepare_enrollment(
                    chain.metadata(),
                    &config,
                    registrar.clone(),
                    user_id,
                    context,
                    checkpoint.hash,
                    &device,
                )
                .await?;
                let pending = profiles::PendingEnrollment {
                    name: name.clone(),
                    genesis_hash,
                    registrar: registrar.clone(),
                    device_wallet: device_wallet.clone(),
                    user_id: user_id.0,
                    pass_account: draft.predicted_account.0,
                    device_id: draft.attestation.device_id.0,
                    pallet: draft.call.pallet,
                    call: draft.call.call,
                    call_bytes: draft.call.bytes,
                    call_hex: draft.call.hex,
                    checkpoint_number: draft.checkpoint_number,
                    checkpoint_hash: draft.checkpoint_hash,
                    valid_through: draft.valid_through,
                };
                store.upsert_pending(pending.clone());
                store.save(profile_path)?;
                pending
            };
            let call = sube::PreparedCall {
                pallet: pending.pallet.clone(),
                call: pending.call.clone(),
                bytes: pending.call_bytes.clone(),
                hex: pending.call_hex.clone(),
            };
            let registrar_signer = wallet_signer(&registrar_profile).await?;
            let receipt = review_pass_transaction(
                &mut chain,
                chain_url,
                &registrar,
                &call,
                &registrar_signer,
                submit,
                yes,
            )
            .await?;
            let Some(receipt) = receipt else {
                println!(
                    "Predicted pass account: 0x{}",
                    hex::encode(pending.pass_account)
                );
                println!(
                    "Pending enrollment retained until #{}",
                    pending.valid_through
                );
                return Ok(());
            };
            if receipt.finalized_block_hash.is_none()
                || !matches!(receipt.dispatch_outcome, sube::DispatchOutcome::Success)
            {
                anyhow::bail!(
                    "registration was not finalized successfully; pending draft was retained"
                );
            }
            store.upsert(profiles::Profile::Pass(profiles::PassProfile {
                name: name.clone(),
                genesis_hash,
                pass_account: pending.pass_account,
                user_id: user_id.0,
                device_id: pending.device_id,
                device_wallet,
                session: None,
            }));
            store.remove_pending(&name);
            store.save(profile_path)?;
            println!("Persisted finalized pass profile {name}");
            Ok(())
        }
        PassCommand::Device { command } => match command {
            DeviceCommand::Add {
                profile,
                device_wallet,
                filter,
                admin_confirm,
                submit,
                yes,
            } => {
                let pass_profile = pass_profile(&store, &profile, genesis_hash)?.clone();
                let current_wallet =
                    wallet_profile(&store, &pass_profile.device_wallet, genesis_hash)?.clone();
                let new_wallet = wallet_profile(&store, &device_wallet, genesis_hash)?.clone();
                let new_signer = wallet_signer(&new_wallet).await?;
                let new_public = new_signer.inner().public();
                let new_device = pass::wallet::WalletDevice::new(
                    new_signer.inner(),
                    new_public.as_ref(),
                    pass::wallet::SignatureType::Sr25519,
                );
                let pass_account = pass::Account(pass_profile.pass_account);
                let request = pass::workflow::AttestationRequest {
                    user_id: pass::HashedUserId(pass_profile.user_id),
                    pass_account,
                    authority_id: config.authority_id,
                    context,
                    block_hash: checkpoint.hash,
                    challenge: pass::enrollment_challenge(&checkpoint.hash, pass_account),
                };
                let attestation = new_device.attest(&request).await?;
                let filter = parse_device_filter(chain.metadata(), &filter)?;
                let call = pass::workflow::prepare_add_device(
                    chain.metadata(),
                    &config,
                    &attestation,
                    &filter,
                    true,
                    admin_confirm,
                )?;

                let current_signer = wallet_signer(&current_wallet).await?;
                let current_public = current_signer.inner().public();
                let current_device = pass::wallet::WalletDevice::new(
                    current_signer.inner(),
                    current_public.as_ref(),
                    pass::wallet::SignatureType::Sr25519,
                );
                let provider = pass::PassAuthorizer::new(
                    &current_device,
                    pass::HashedUserId(pass_profile.user_id),
                    config.authority_id,
                    context,
                    checkpoint.hash,
                )
                .with_challenger(config.challenger);
                let authorizer = pass::PassAuthenticator::new(
                    pass_account,
                    pass::DeviceId(pass_profile.device_id),
                    provider,
                );
                let receipt = review_pass_transaction(
                    &mut chain,
                    chain_url,
                    &profile,
                    &call,
                    &authorizer,
                    submit,
                    yes,
                )
                .await?;
                if let Some(receipt) = receipt
                    && receipt.finalized_block_hash.is_some()
                    && matches!(receipt.dispatch_outcome, sube::DispatchOutcome::Success)
                {
                    println!("Added device 0x{}", hex::encode(attestation.device_id.0));
                }
                Ok(())
            }
            DeviceCommand::Remove {
                profile,
                device_id,
                submit,
                yes,
            } => {
                let pass_profile = pass_profile(&store, &profile, genesis_hash)?.clone();
                let device_id = pass::DeviceId(profiles::parse_hash(&device_id, "device id")?);
                if device_id.0 == pass_profile.device_id {
                    eprintln!(
                        "Warning: removing the locally configured device may leave this profile unusable."
                    );
                }
                let call =
                    pass::workflow::prepare_remove_device(chain.metadata(), &config, device_id)?;
                let current_wallet =
                    wallet_profile(&store, &pass_profile.device_wallet, genesis_hash)?.clone();
                let current_signer = wallet_signer(&current_wallet).await?;
                let current_public = current_signer.inner().public();
                let current_device = pass::wallet::WalletDevice::new(
                    current_signer.inner(),
                    current_public.as_ref(),
                    pass::wallet::SignatureType::Sr25519,
                );
                let provider = pass::PassAuthorizer::new(
                    &current_device,
                    pass::HashedUserId(pass_profile.user_id),
                    config.authority_id,
                    context,
                    checkpoint.hash,
                )
                .with_challenger(config.challenger);
                let authorizer = pass::PassAuthenticator::new(
                    pass::Account(pass_profile.pass_account),
                    pass::DeviceId(pass_profile.device_id),
                    provider,
                );
                review_pass_transaction(
                    &mut chain,
                    chain_url,
                    &profile,
                    &call,
                    &authorizer,
                    submit,
                    yes,
                )
                .await?;
                Ok(())
            }
        },
        PassCommand::Session { .. } => unreachable!("handled before connecting"),
    }
}

#[cfg(feature = "pass")]
fn wallet_profile<'a>(
    store: &'a profiles::Profiles,
    name: &str,
    genesis_hash: [u8; 32],
) -> Result<&'a profiles::WalletProfile> {
    match store.find(name) {
        Some(profiles::Profile::Wallet(profile)) if profile.genesis_hash == genesis_hash => {
            Ok(profile)
        }
        Some(profiles::Profile::Wallet(_)) => {
            anyhow::bail!("wallet profile genesis hash does not match the connected chain")
        }
        _ => anyhow::bail!("wallet profile {name:?} not found"),
    }
}

#[cfg(feature = "pass")]
fn pass_profile<'a>(
    store: &'a profiles::Profiles,
    name: &str,
    genesis_hash: [u8; 32],
) -> Result<&'a profiles::PassProfile> {
    match store.find(name) {
        Some(profiles::Profile::Pass(profile)) if profile.genesis_hash == genesis_hash => {
            Ok(profile)
        }
        Some(profiles::Profile::Pass(_)) => {
            anyhow::bail!("pass profile genesis hash does not match the connected chain")
        }
        _ => anyhow::bail!("pass profile {name:?} not found"),
    }
}

#[cfg(feature = "pass")]
fn parse_device_filter(
    metadata: &sube::Metadata,
    value: &str,
) -> Result<pass::workflow::DeviceFilter> {
    if value.eq_ignore_ascii_case("admin") {
        return Ok(pass::workflow::DeviceFilter::Admin);
    }
    if let Some(calls) = value.strip_prefix("calls:") {
        let policy = pass::SessionPolicy::resolve(
            metadata,
            pass::session::SessionPolicyRequest::Calls(
                calls.split(',').map(str::to_owned).collect(),
            ),
        )?;
        if let pass::SessionPolicy::Calls(calls) = policy {
            return Ok(pass::workflow::DeviceFilter::Calls(
                calls
                    .into_iter()
                    .map(|call| (call.pallet, call.call))
                    .collect(),
            ));
        }
    }
    if let Some(pallets) = value.strip_prefix("pallets:") {
        let policy = pass::SessionPolicy::resolve(
            metadata,
            pass::session::SessionPolicyRequest::Pallets(
                pallets.split(',').map(str::to_owned).collect(),
            ),
        )?;
        if let pass::SessionPolicy::Pallets(pallets) = policy {
            return Ok(pass::workflow::DeviceFilter::Pallets(pallets));
        }
    }
    anyhow::bail!("filter must be calls:pallet/call,..., pallets:pallet,..., or admin")
}

#[cfg(feature = "pass")]
fn parse_session_policy(metadata: &sube::Metadata, value: &str) -> Result<pass::SessionPolicy> {
    if value.eq_ignore_ascii_case("admin") {
        anyhow::bail!("Admin sessions are not permitted");
    }
    if let Some(calls) = value.strip_prefix("calls:") {
        return Ok(pass::SessionPolicy::resolve(
            metadata,
            pass::session::SessionPolicyRequest::Calls(
                calls.split(',').map(str::to_owned).collect(),
            ),
        )?);
    }
    if let Some(pallets) = value.strip_prefix("pallets:") {
        return Ok(pass::SessionPolicy::resolve(
            metadata,
            pass::session::SessionPolicyRequest::Pallets(
                pallets.split(',').map(str::to_owned).collect(),
            ),
        )?);
    }
    if let Some(spend) = value.strip_prefix("spend:") {
        let mut parts = spend.splitn(3, ':');
        let pallet = parts.next().unwrap_or_default();
        let limit = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("spend policy is missing a limit"))?
            .parse::<u128>()?;
        let calls = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("spend policy is missing calls"))?;
        return Ok(pass::SessionPolicy::resolve(
            metadata,
            pass::session::SessionPolicyRequest::Spend {
                pallet: pallet.into(),
                calls: calls.split(',').map(str::to_owned).collect(),
                limit,
            },
        )?);
    }
    anyhow::bail!(
        "session policy must be calls:pallet/call,..., pallets:pallet,..., or spend:pallet:limit:call,..."
    )
}

#[cfg(feature = "pass")]
async fn review_pass_transaction(
    chain: &mut sube::Sube,
    chain_url: &str,
    authorizer_name: &str,
    call: &sube::PreparedCall,
    authorizer: &(impl sube::ExtrinsicAssembler + ?Sized),
    submit: bool,
    yes: bool,
) -> Result<Option<sube::TransactionReceipt>> {
    let extrinsic = chain
        .build_transaction(call, authorizer, sube::TransactionOptions::default())
        .await?;
    let report = chain.inspect_transaction(&extrinsic).await?;
    if matches!(report.validity, Some(sube::TransactionValidity::Invalid(_))) {
        anyhow::bail!("transaction validation reports known-invalid; submission blocked");
    }
    print_review(chain_url, authorizer_name, &extrinsic, &report);
    if !submit {
        return Ok(None);
    }
    if !yes && !confirm_submission()? {
        anyhow::bail!("submission canceled; no transaction was submitted");
    }
    chain
        .submit_transaction(&extrinsic, sube::WaitFor::Finalized)
        .await
        .map(Some)
        .map_err(Into::into)
}

fn print_response(response: &sube::Response) -> Result<()> {
    use sube::Response;
    match response {
        Response::None => println!("(none)"),
        Response::Value(entry, meta) => {
            println!("{}", entry.to_text(&meta.registry)?);
        }
        Response::ValueSet(items, meta) => {
            for (keys, value) in items {
                let key_strs: Vec<String> = keys
                    .iter()
                    .filter_map(|k| k.to_text(&meta.registry).ok())
                    .collect();
                let key_display = key_strs.join(", ");
                match value {
                    Some(v) => println!("[{key_display}] {}", v.to_text(&meta.registry)?),
                    None => println!("[{key_display}] (none)"),
                }
            }
        }
        Response::Meta(meta) => {
            for p in &meta.pallets {
                println!("{}", p.name);
            }
        }
        Response::Void => {}
    }
    Ok(())
}

async fn oneshot(chain: &str, path: &str) -> Result<()> {
    eprintln!("Connecting to {chain}...");
    let mut chain = sube::Sube::connect(chain).await?;
    let response = chain.query(path).await?;
    print_response(&response)
}

async fn watch(chain_url: &str, path: &str) -> Result<()> {
    eprintln!("Connecting to {chain_url}...");
    let mut chain = sube::Sube::connect(chain_url).await?;

    // Print initial value
    let response = chain.query(path).await?;
    print_response(&response)?;
    let mut prev_raw: Vec<u8> = match &response {
        sube::Response::Value(entry, _) => entry.data.clone(),
        _ => vec![],
    };

    loop {
        // Wait for a new block and query at that specific block
        let hash = loop {
            match chain.next_event().await? {
                sube::ChainEvent::NewBlock { hash, .. } => break hash,
                _ => continue,
            }
        };

        let response = chain.query_at_hash(path, &hash).await?;
        let current_raw = match &response {
            sube::Response::Value(entry, _) => entry.data.clone(),
            _ => vec![],
        };

        if current_raw != prev_raw {
            print_response(&response)?;
            prev_raw = current_raw;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_submission_is_opt_in() {
        let cli = Cli::try_parse_from(["sube", "tx", "system/remark", "--body", "(remark:0x01)"])
            .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Tx { submit: false, .. })
        ));
    }

    #[test]
    fn submit_without_a_profile_fails_before_connecting() {
        let error = smol::block_on(tx(
            "wss://endpoint-that-must-not-be-contacted.invalid",
            std::path::Path::new("/tmp/sube-no-profile-test.json"),
            "system/remark",
            "(remark:0x01)",
            BodyFormat::Text,
            TxBehavior {
                json: false,
                submit: true,
                yes: true,
                best: false,
            },
        ))
        .unwrap_err();
        assert!(error.to_string().contains("no transaction was submitted"));
    }

    #[test]
    fn transaction_body_can_come_from_whole_json_or_stdin_flags() {
        let json = Cli::try_parse_from([
            "sube",
            "tx",
            "system/remark",
            "--body",
            r#"{"remark":"0x01"}"#,
            "--body-format",
            "json",
        ])
        .unwrap();
        assert!(matches!(
            json.command,
            Some(Command::Tx {
                body_format: BodyFormat::Json,
                ..
            })
        ));

        let stdin = Cli::try_parse_from(["sube", "tx", "system/remark", "--body-stdin"]).unwrap();
        assert!(matches!(
            stdin.command,
            Some(Command::Tx {
                body_stdin: true,
                ..
            })
        ));
    }

    #[cfg(feature = "wallet")]
    #[test]
    fn review_artifact_contains_both_identities_and_no_secret_fields() {
        let extrinsic = sube::EncodedExtrinsic {
            bytes: vec![1, 2],
            hex: "0x0102".into(),
            call: sube::PreparedCall {
                pallet: "Balances".into(),
                call: "transfer".into(),
                bytes: vec![5, 0],
                hex: "0x0500".into(),
            },
            checkpoint_hash: [3; 32],
            checkpoint_number: 10,
            expires_at: Some(64),
            genesis_hash: [4; 32],
            spec_version: 5,
            transaction_version: 6,
            nonce: 7,
            authorization: sube::AuthorizationSummary {
                signing_account: vec![8; 32],
                nonce_account: vec![9; 32],
                scheme: Some("Sr25519".into()),
            },
        };
        let artifact = transaction_artifact(
            "wss://example.invalid",
            "alice",
            &extrinsic,
            &sube::TransactionReport::default(),
        );
        assert_eq!(
            artifact["signingAccount"],
            format!("0x{}", hex::encode([8; 32]))
        );
        assert_eq!(
            artifact["nonceAccount"],
            format!("0x{}", hex::encode([9; 32]))
        );
        let encoded = artifact.to_string();
        assert!(!encoded.contains("mnemonic"));
        assert!(!encoded.contains("secret"));
    }

    #[cfg(feature = "pass")]
    #[test]
    fn pass_commands_require_explicit_user_and_device_profiles() {
        let cli = Cli::try_parse_from([
            "sube",
            "pass",
            "enroll",
            "--user-id",
            &"11".repeat(32),
            "--registrar",
            "sponsor",
            "--device-wallet",
            "device",
            "--name",
            "my-pass",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Pass {
                command: PassCommand::Enroll { submit: false, .. }
            })
        ));
    }

    #[cfg(feature = "pass")]
    #[test]
    fn on_chain_policy_comparison_is_structurally_exact() {
        let policy = pass::SessionPolicy::Calls(vec![pass::session::CallIndex {
            pallet: 5,
            call: 12,
        }]);
        assert!(policy_text_matches(
            "Session { filter: Filter::Calls([(5, 12)]), expires: 99 }",
            &policy
        ));
        assert!(!policy_text_matches(
            "Session { filter: Filter::Calls([(5, 1), (2, 9)]), expires: 99 }",
            &policy
        ));
    }
}
