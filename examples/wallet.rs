use clap::{App, Arg};
use libwallet::{self, sr25519::Pair, Pair as _, Result, SimpleVault};

type Wallet = libwallet::Wallet<SimpleVault<Pair>>;

#[async_std::main]
async fn main() -> Result<()> {
    let matches = App::new("Wallet Generator")
        .version("0.1.0")
        .author("Virto Team <we@virto.team>")
        .about("Generates Wallet Account")
        .arg(
            Arg::with_name("seed")
                .short("s")
                .long("from-seed")
                .value_name("MNEMONIC")
                .help("Generates a wallet address from mnemonic."),
        )
        .arg(
            Arg::with_name("network")
                .short("n")
                .long("network")
                .value_name("NETWORK")
                .help("Formats the address to specified network."),
        )
        .get_matches();

    let mut wallet = get_wallet(matches.value_of("seed"));
    wallet.unlock("").await?;
    let _network: &str = matches.value_of("network").unwrap_or("substrate");

    println!("Public key (SS58): {}", wallet);
    Ok(())
}

fn get_wallet(seed: Option<&str>) -> Wallet {
    let mnemonic = match seed {
        Some(mnemonic) => mnemonic.into(),
        None => Pair::generate_with_phrase(None).1,
    };

    println!("Secret Key: \"{}\"", mnemonic);
    SimpleVault::<Pair>::from(&*mnemonic).into()
}
