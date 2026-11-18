use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Read;
#[cfg(feature = "wallet")]
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

#[cfg(feature = "pass")]
mod device;
mod profiles;
#[cfg(feature = "wallet")]
mod signers;
mod tui;
#[cfg_attr(not(feature = "wallet"), allow(dead_code))]
mod workflow;

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
    /// Import a wallet from the mnemonic read on stdin. Its //default
    /// sr25519 account is derived and the mnemonic kept in the OS secure
    /// store.
    #[cfg(feature = "wallet")]
    AddWallet {
        name: String,
        /// Genesis hash of the chain the profile signs for.
        #[arg(long)]
        genesis: String,
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
        /// Device to enroll: wallet:PROFILE or webauthn:RP_ID@ORIGIN.
        #[arg(long)]
        device: profiles::DeviceProviderProfile,
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
        /// Device to add: wallet:PROFILE or webauthn:RP_ID@ORIGIN.
        #[arg(long)]
        device: profiles::DeviceProviderProfile,
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
    ensure_confirmable(behavior.submit, behavior.yes)?;
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
    if let Some(signed) =
        workflow::prepare_with_active_profile(&mut chain, chain_url, profile_path, &prepared)
            .await?
    {
        let staged = workflow::Staged {
            prepared: signed,
            notes: Vec::new(),
            effect: None,
        };
        return finish(&mut chain, profile_path, staged, behavior).await;
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
                "callHex": prepared.hex(),
                "submitted": false,
            })
        );
    } else {
        println!("{}", prepared.hex());
    }
}

/// Print the review of `staged` to `out`, then decide whether to submit it:
/// never for a dry run, never when the chain reports it invalid, and
/// otherwise once `confirm` agrees (or `--yes` was given).
#[cfg(feature = "wallet")]
fn review(
    staged: &workflow::Staged,
    behavior: TxBehavior,
    out: &mut impl Write,
    confirm: impl FnOnce() -> Result<bool>,
) -> Result<bool> {
    if behavior.json {
        for note in &staged.notes {
            eprintln!("{note}");
        }
        writeln!(out, "{}", staged.prepared.artifact())?;
    } else {
        write!(out, "{}", staged.review_text())?;
    }
    out.flush()?;
    if !behavior.submit {
        return Ok(false);
    }
    if staged.prepared.known_invalid() {
        anyhow::bail!("the chain reports this transaction invalid; no transaction was submitted");
    }
    if !behavior.yes && !confirm()? {
        anyhow::bail!("submission canceled; no transaction was submitted");
    }
    Ok(true)
}

/// Review `staged`, then submit it if asked and apply its local change once
/// it finalizes.
#[cfg(feature = "wallet")]
async fn finish(
    chain: &mut sube::Sube,
    profile_path: &std::path::Path,
    staged: workflow::Staged,
    behavior: TxBehavior,
) -> Result<()> {
    if !review(
        &staged,
        behavior,
        &mut std::io::stdout(),
        confirm_submission,
    )? {
        return Ok(());
    }
    let wait_for = if behavior.best {
        sube::WaitFor::BestBlock
    } else {
        sube::WaitFor::Finalized
    };
    let receipt = workflow::submit_transaction(chain, &staged.prepared, wait_for).await?;
    println!("{}", workflow::receipt_json(&receipt));
    #[cfg(feature = "pass")]
    if let Some(effect) = staged.effect {
        let message = effect.finalized_message();
        workflow::persist_effect(chain, profile_path, &receipt, effect).await?;
        println!("{message}");
    }
    #[cfg(not(feature = "pass"))]
    let _ = profile_path;
    Ok(())
}

/// Fail before anything is built or any device is prompted when a
/// submission would need an interactive confirmation that cannot happen.
fn ensure_confirmable(submit: bool, yes: bool) -> Result<()> {
    if submit && !yes && !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        anyhow::bail!("interactive confirmation requires a terminal; use --yes");
    }
    Ok(())
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
            Ok(())
        }
        #[cfg(feature = "wallet")]
        ProfileCommand::AddWallet { name, genesis } => {
            let genesis_hash = profiles::parse_hash(&genesis, "genesis hash")?;
            let mut mnemonic = zeroize::Zeroizing::new(String::new());
            std::io::stdin().read_to_string(&mut mnemonic)?;
            let store =
                workflow::import_wallet_profile(path, genesis_hash, name.clone(), mnemonic.trim())?;
            if let Some(profiles::Profile::Wallet(profile)) = store.find(&name) {
                println!(
                    "Added wallet profile {name} 0x{}",
                    hex::encode(profile.account)
                );
            }
            Ok(())
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
    let genesis_hash = workflow::chain_genesis(&mut chain).await?;
    #[cfg(feature = "pass")]
    if let Some(profiles::Profile::Pass(profile)) =
        profiles::Profiles::load(profile_path)?.find(name)
    {
        let policy = session_policy
            .map(str::to_owned)
            .or_else(|| {
                profile
                    .session
                    .as_ref()
                    .map(|session| session.policy.clone())
            })
            .ok_or_else(|| {
                anyhow::anyhow!("pass connect requires --session-policy calls:... or pallets:...")
            })?;
        ensure_confirmable(true, yes)?;
        let preparation = workflow::prepare_pass_session(
            &mut chain,
            chain_url,
            profile_path,
            genesis_hash,
            name,
            &policy,
            session_duration,
        )
        .await?;
        if let workflow::SessionPreparation::Review(staged) = preparation {
            let behavior = TxBehavior {
                json: false,
                submit: true,
                yes,
                best: false,
            };
            finish(&mut chain, profile_path, *staged, behavior).await?;
        }
        println!(
            "Connected profile {name} to 0x{}",
            hex::encode(genesis_hash)
        );
        return Ok(());
    }
    #[cfg(not(feature = "pass"))]
    let _ = (session_policy, session_duration, yes);
    #[cfg(feature = "wallet")]
    workflow::activate_existing_profile(&mut chain, profile_path, genesis_hash, name).await?;
    #[cfg(not(feature = "wallet"))]
    {
        let mut store = profiles::Profiles::load(profile_path)?;
        store.activate(name, genesis_hash)?;
        store.save(profile_path)?;
    }
    println!(
        "Connected profile {name} to 0x{}",
        hex::encode(genesis_hash)
    );
    Ok(())
}

#[cfg(feature = "pass")]
async fn pass_command(
    chain_url: &str,
    profile_path: &std::path::Path,
    command: PassCommand,
) -> Result<()> {
    if let PassCommand::Session {
        command: SessionCommand::Forget { profile },
    } = &command
    {
        workflow::forget_pass_session(profile_path, None, profile)?;
        println!("{}", pass::session::FORGET_SESSION_WARNING);
        return Ok(());
    }

    let (submit, yes) = match &command {
        PassCommand::Enroll { submit, yes, .. }
        | PassCommand::Device {
            command:
                DeviceCommand::Add { submit, yes, .. } | DeviceCommand::Remove { submit, yes, .. },
        } => (*submit, *yes),
        PassCommand::Session { .. } => (false, false),
    };
    ensure_confirmable(submit, yes)?;

    let mut chain = sube::Sube::connect(chain_url).await?;
    let genesis_hash = workflow::chain_genesis(&mut chain).await?;
    let (staged, submit, yes) = match command {
        PassCommand::Enroll {
            user_id,
            registrar,
            device,
            name,
            submit,
            yes,
        } => {
            let request = workflow::EnrollmentRequest {
                name,
                user_id,
                registrar,
                device,
            };
            let staged = workflow::prepare_enrollment(
                &mut chain,
                chain_url,
                profile_path,
                genesis_hash,
                request,
            )
            .await?;
            (staged, submit, yes)
        }
        PassCommand::Device {
            command:
                DeviceCommand::Add {
                    profile,
                    device,
                    filter,
                    admin_confirm,
                    submit,
                    yes,
                },
        } => {
            let request = workflow::DeviceAdditionRequest {
                profile,
                device,
                filter,
                admin_confirmed: admin_confirm,
            };
            let staged = workflow::prepare_device_addition(
                &mut chain,
                chain_url,
                profile_path,
                genesis_hash,
                request,
            )
            .await?;
            (staged, submit, yes)
        }
        PassCommand::Device {
            command:
                DeviceCommand::Remove {
                    profile,
                    device_id,
                    submit,
                    yes,
                },
        } => {
            let request = workflow::DeviceRemovalRequest {
                profile,
                device_id: profiles::parse_hash(&device_id, "device id")?,
            };
            let staged = workflow::prepare_device_removal(
                &mut chain,
                chain_url,
                profile_path,
                genesis_hash,
                request,
            )
            .await?;
            (staged, submit, yes)
        }
        PassCommand::Session { .. } => unreachable!("handled before connecting"),
    };
    let behavior = TxBehavior {
        json: false,
        submit,
        yes,
        best: false,
    };
    finish(&mut chain, profile_path, staged, behavior).await
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

    #[cfg(feature = "pass")]
    struct E2eCleanup {
        profile_path: std::path::PathBuf,
        secure_entries: Vec<String>,
    }

    #[cfg(feature = "pass")]
    impl Drop for E2eCleanup {
        fn drop(&mut self) {
            use libwallet::MutableKeyStore;

            for entry in &self.secure_entries {
                let mut keys = libwallet::vault::OSKeyring::<()>::new(entry, None);
                let _ = keys.delete();
            }
            let _ = std::fs::remove_file(&self.profile_path);
        }
    }

    #[cfg(feature = "wallet")]
    fn staged(validity: Option<sube::TransactionValidity>) -> workflow::Staged {
        let transaction = sube::EncodedExtrinsic {
            bytes: vec![1, 2],
            call: sube::PreparedCall {
                pallet: "System".into(),
                call: "remark".into(),
                bytes: vec![0, 0],
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
            extensions: vec![],
        };
        workflow::Staged {
            prepared: workflow::PreparedTransaction {
                transaction,
                report: sube::TransactionReport {
                    validity,
                    ..Default::default()
                },
                authorizer: "alice".into(),
                chain_url: "wss://example.invalid".into(),
            },
            notes: vec!["Predicted pass account: 0x01".into()],
            effect: None,
        }
    }

    /// A writer the confirmation prompt can inspect while `review` holds it.
    #[cfg(feature = "wallet")]
    struct Shared<'a>(&'a std::cell::RefCell<Vec<u8>>);

    #[cfg(feature = "wallet")]
    impl std::io::Write for Shared<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[cfg(feature = "wallet")]
    #[test]
    fn the_review_is_printed_before_confirmation_is_asked() {
        let output = std::cell::RefCell::new(Vec::new());
        let behavior = TxBehavior {
            json: false,
            submit: true,
            yes: false,
            best: false,
        };
        let mut asked = false;
        let error = review(
            &staged(Some(sube::TransactionValidity::Valid)),
            behavior,
            &mut Shared(&output),
            || {
                asked = true;
                let printed = String::from_utf8(output.borrow().clone()).unwrap();
                assert!(printed.starts_with("Predicted pass account: 0x01\n\n"));
                assert!(printed.contains("Call: System::remark"));
                assert!(printed.contains("Extrinsic hex: 0x0102"));
                Ok(false)
            },
        )
        .unwrap_err();
        assert!(asked);
        assert!(error.to_string().contains("no transaction was submitted"));
    }

    #[cfg(feature = "wallet")]
    #[test]
    fn known_invalid_transactions_are_reviewed_but_never_submitted() {
        let invalid = || staged(Some(sube::TransactionValidity::Invalid("0x0001".into())));
        let mut behavior = TxBehavior {
            json: false,
            submit: false,
            yes: false,
            best: false,
        };
        let mut output = Vec::new();
        let never = || -> Result<bool> { panic!("must not ask to submit") };
        assert!(!review(&invalid(), behavior, &mut output, never).unwrap());
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Invalid(\"0x0001\")")
        );

        behavior.submit = true;
        behavior.yes = true;
        let error = review(&invalid(), behavior, &mut Vec::new(), never).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("reports this transaction invalid")
        );
    }

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
            "--device",
            "wallet:device",
            "--name",
            "my-pass",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Some(Command::Pass {
                command: PassCommand::Enroll {
                    submit: false,
                    device: profiles::DeviceProviderProfile::SubstrateKey { .. },
                    ..
                }
            })
        ));
    }

    #[cfg(feature = "pass")]
    #[test]
    #[ignore = "requires SUBE_E2E_URL and disposable funded live-chain credentials"]
    fn live_pass_enrollment_session_and_submission() -> Result<()> {
        smol::block_on(async {
            let Ok(chain_url) = std::env::var("SUBE_E2E_URL") else {
                eprintln!("SUBE_E2E_URL is unset; skipping live-chain pass test");
                return Ok(());
            };
            let registrar_mnemonic = std::env::var("SUBE_E2E_REGISTRAR_MNEMONIC")
                .map_err(|_| anyhow::anyhow!("SUBE_E2E_REGISTRAR_MNEMONIC is required"))?;
            let device_mnemonic = std::env::var("SUBE_E2E_DEVICE_MNEMONIC")
                .map_err(|_| anyhow::anyhow!("SUBE_E2E_DEVICE_MNEMONIC is required"))?;
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let registrar_name = format!("e2e-registrar-{unique}");
            let device_name = format!("e2e-device-{unique}");
            let pass_name = format!("e2e-pass-{unique}");
            let profile_path = std::env::temp_dir().join(format!(
                "sube-live-e2e-{}-{unique}.json",
                std::process::id()
            ));
            let mut cleanup = E2eCleanup {
                profile_path: profile_path.clone(),
                secure_entries: vec![
                    format!("wallet-profile-v1-{registrar_name}"),
                    format!("wallet-profile-v1-{device_name}"),
                ],
            };

            let mut chain = sube::Sube::connect(&chain_url).await?;
            let genesis_hash = workflow::chain_genesis(&mut chain).await?;
            workflow::import_wallet_profile(
                &profile_path,
                genesis_hash,
                registrar_name.clone(),
                &registrar_mnemonic,
            )?;
            workflow::import_wallet_profile(
                &profile_path,
                genesis_hash,
                device_name.clone(),
                &device_mnemonic,
            )?;

            let mut user_id = [0u8; 32];
            user_id[..16].copy_from_slice(&unique.to_le_bytes());
            user_id[16..].copy_from_slice(&genesis_hash[..16]);
            let enrollment = workflow::prepare_enrollment(
                &mut chain,
                &chain_url,
                &profile_path,
                genesis_hash,
                workflow::EnrollmentRequest {
                    name: pass_name.clone(),
                    user_id: hex::encode(user_id),
                    registrar: registrar_name,
                    device: profiles::DeviceProviderProfile::SubstrateKey {
                        wallet: device_name,
                    },
                },
            )
            .await?;
            let receipt = workflow::submit_transaction(
                &mut chain,
                &enrollment.prepared,
                sube::WaitFor::Finalized,
            )
            .await?;
            assert!(matches!(
                receipt.dispatch_outcome,
                sube::DispatchOutcome::Success
            ));
            let effect = enrollment.effect.expect("enrollment changes the profile");
            workflow::apply_finalized_effect(&profile_path, &receipt, effect)?;

            let session = workflow::prepare_pass_session(
                &mut chain,
                &chain_url,
                &profile_path,
                genesis_hash,
                &pass_name,
                "calls:System/remark",
                None,
            )
            .await?;
            let workflow::SessionPreparation::Review(staged) = session else {
                anyhow::bail!("new pass profile unexpectedly reused a session")
            };
            let receipt = workflow::submit_transaction(
                &mut chain,
                &staged.prepared,
                sube::WaitFor::Finalized,
            )
            .await?;
            assert!(matches!(
                receipt.dispatch_outcome,
                sube::DispatchOutcome::Success
            ));
            let effect = staged.effect.expect("a session changes the profile");
            let stored = workflow::apply_finalized_effect(&profile_path, &receipt, effect)?;
            let profile = workflow::pass_profile(&stored, &pass_name, genesis_hash)?.clone();
            let session = profile
                .session
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("finalized session was not persisted"))?;
            cleanup.secure_entries.push(profile.session_entry());
            let signer =
                signers::load_session_signer(&profile.session_entry(), session.account).await?;
            let policy = workflow::parse_session_policy(chain.metadata(), &session.policy)?;
            let authorizer =
                pass::SessionAuthorizer::new(pass::Account(profile.pass_account), signer, policy)?;

            let body = sube::Text("(remark:0x737562652d653265)");
            let disallowed = chain.prepare_call("System/remark_with_event", &body)?;
            let error = chain
                .build_transaction(
                    &disallowed,
                    &authorizer,
                    sube::TransactionOptions::default(),
                )
                .await
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("outside the local session policy")
            );

            let allowed = chain.prepare_call("System/remark", &body)?;
            let prepared = workflow::prepare_transaction(
                &mut chain,
                &chain_url,
                &allowed,
                &pass_name,
                &authorizer,
                sube::TransactionOptions::default(),
            )
            .await?;
            let receipt =
                workflow::submit_transaction(&mut chain, &prepared, sube::WaitFor::Finalized)
                    .await?;
            assert!(matches!(
                receipt.dispatch_outcome,
                sube::DispatchOutcome::Success
            ));
            assert!(receipt.finalized_block_hash.is_some());
            assert!(receipt.extrinsic_index.is_some());
            Ok(())
        })
    }
}
