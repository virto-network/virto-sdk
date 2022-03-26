use libwallet::{self, sr25519::Pair, SimpleVault, Result};

type Wallet = libwallet::Wallet<SimpleVault<Pair>>;

const MNEMONIC: &str = "caution juice atom organ advance problem want pledge someone senior holiday very";

#[async_std::test]
async fn test_wallet_creation() {
    // key example taken from https://docs.substrate.io/v3/tools/subkey/#hd-key-derivation
    let key = get_address_from_mnemonic(MNEMONIC).await.unwrap();

    assert_eq!(key, "5Gv8YYFu8H1btvmrJy9FjjAWfb99wrhV3uhPFoNEr918utyR");
}

async fn get_address_from_mnemonic(mnemonic: &str) -> Result<String> {
    let vault = SimpleVault::<Pair>::from(mnemonic);
    let mut wallet = Wallet::new(vault).unlock(()).await?;
    let account = wallet.switch_default_network("substrate")?;
    Ok(account.to_string())
}

#[async_std::test]
async fn test_account_derivation() {
    let derivation_path = "/polkadot/0";

    let key = get_address_from_mnemonic_and_derivation_path(MNEMONIC, derivation_path).await.unwrap();
    assert_eq!(key, "5FRjccB7s9fbMu4pwQhYng2quQnKYkCHXRUCMBRwL7Pzj8FX");
}
    

async fn get_address_from_mnemonic_and_derivation_path(mnemonic: &str, derivation_path: &str) -> Result<String> {
    let vault: SimpleVault::<Pair> = mnemonic.into();
    let mut wallet = Wallet::new(vault).unlock(()).await?;
    wallet.switch_default_network("substrate")?;
    let subaccount = wallet.create_sub_account("test", derivation_path);
    subaccount.map(|a| a.to_string())
}


#[async_std::test]
async fn test_serialize_root_account() {
    let vault = SimpleVault::<Pair>::from(MNEMONIC);
    let mut wallet = Wallet::new(vault).unlock(()).await.unwrap();
    let account = wallet.switch_default_network("substrate").unwrap();
    
    let account_json = serde_json::to_string(&account).unwrap();
    assert_eq!(account_json, "{\"account_type\":\"root\",\"network\":{\"Substrate\":42}}");
}

#[async_std::test]
async fn test_serialize_sub_account() {
    let vault = SimpleVault::<Pair>::from(MNEMONIC);
    let mut wallet = Wallet::new(vault).unlock(()).await.unwrap();
    wallet.switch_default_network("substrate").unwrap();
    let derivation_path = "/polkadot/0";
    let subaccount = wallet.create_sub_account("test", derivation_path).unwrap();

    let account_json = serde_json::to_string(&subaccount).unwrap();
    assert_eq!(account_json, "{\"account_type\":\"sub\",\"name\":\"test\",\"derivation_path\":\"/polkadot/0\",\"network\":{\"Substrate\":42}}");
}
