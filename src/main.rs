extern crate clap;
use clap::{App, Arg};
use libwallet::{
     self,
     sr25519::{Pair, Public},
     Pair as _, SimpleVault, Wallet,
};
use sp_core::crypto::Ss58Codec;

#[async_std::main]
async fn main() {
     let matches = App::new("Wallet Generator")
          .version("0.1.0")
          .author("Virto Team <we@virto.team>")
          .about("Generates Wallet Account")
          .arg(Arg::with_name("seed")
               .short("s")
               .long("from-seed")
               .value_name("MNEMONIC")
               .help("Generates a wallet address from mnemonic."))
          .arg(Arg::with_name("network")
               .short("n")
               .long("network")
               .value_name("NETWORK")
               .help("Formats the address to specified network."))
          .get_matches();

     let pub_address = get_pub_address(matches.value_of("seed")).await;
     let network: &str = matches.value_of("network").unwrap_or("substrate");

     let address: String = pub_address
          .to_ss58check_with_version(network.parse().unwrap_or_else(|_| Default::default()));
     println!("Public key (SS58): {}", address);
}

async fn get_pub_address(seed: Option<&str>) -> Public {
     let vault = match seed {
          Some(mnemonic) => {
               println!("Secret Key: \"{}\"", mnemonic);
               let vault = SimpleVault::<Pair>::from(mnemonic);
               vault
          }
          None => {
               let mnemonic: String = Pair::generate_with_phrase(None).1;
               println!("Secret Key: \"{}\"", mnemonic);
               let vault = SimpleVault::<Pair>::from(mnemonic.as_str());
               vault
          }
     };

     let mut wallet = Wallet::from(vault);
     wallet.unlock("").await.unwrap();
     let public_add = wallet.root_account().unwrap().public();
     public_add
}
