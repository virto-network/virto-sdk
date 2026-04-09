//! Sign a message with a connected Trezor device.
//!
//! Run with:
//!   cargo run --example trezor_sign --features trezor
//!
//! On Linux you may need udev rules for non-root access:
//!   SUBSYSTEM=="usb", ATTR{idVendor}=="534c", MODE="0660", TAG+="uaccess"
//!   SUBSYSTEM=="usb", ATTR{idVendor}=="1209", ATTR{idProduct}=="53c1", MODE="0660", TAG+="uaccess"

use libwallet::transport::trezor::{TrezorSigner, UsbTransport};
use libwallet::Signer;

#[async_std::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let usb = UsbTransport::open()
        .map_err(|e| format!("No Trezor found: {e}"))?;

    // Bitcoin BIP44 path — change to m/44'/60'/0'/0/0 for Ethereum
    let path = "m/44'/0'/0'/0/0";
    let signer = TrezorSigner::new(usb, path)
        .map_err(|e| format!("Bad path: {e}"))?;

    signer.init().await.map_err(|e| format!("Init failed: {e}"))?;

    let message = b"Hello from libwallet!";
    println!("Path:    {path}");
    println!("Message: {:?}", core::str::from_utf8(message).unwrap());
    println!("Confirm on your Trezor...");

    let sig = signer.sign_msg(message).await?;
    print!("Signature: ");
    for b in sig.as_ref() {
        print!("{b:02x}");
    }
    println!(" ({} bytes)", sig.as_ref().len());

    Ok(())
}
