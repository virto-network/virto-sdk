use clap::{App, Arg};
use libwallet::{self, vault, Mnemonic};

type Wallet = libwallet::Wallet<vault::Simple>;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let _network: &str = matches.value_of("network").unwrap_or("substrate");

    let phrase = match matches.value_of("seed") {
        Some(mnemonic) => mnemonic.parse().unwrap(),
        None => {
            let entropy = [0; 32];
            Mnemonic::from_entropy(entropy).unwrap()
        }
    };
    println!("Secret Key: \"{}\"", &phrase);

    let mut wallet = Wallet::new(vault::Simple::from_phrase(phrase));
    wallet.unlock(()).await?;
    let account = wallet.default_account();

    println!("Public key: {}", account);
    Ok(())
}
