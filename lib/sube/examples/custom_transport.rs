//! Using sube with a custom HTTP transport.
//!
//! This shows how to plug in any HTTP client — works in std, no_std, embassy, WASM.
//! You provide the async POST function, sube handles JSON-RPC and SCALE decoding.
//!
//! Run with: cargo run --example custom_transport --features wss,json

use sube::{Backend, HttpTransport, RpcClient};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    smol::block_on(async {
        // On embedded, replace this with your actual HTTP client (reqwless, embassy-net, etc.)
        //
        // ```
        // let transport = HttpTransport::new("http://10.0.0.1:9933", |url, body| async move {
        //     let response = my_http_client.post(url, &body).await?;
        //     Ok(response.body().to_vec())
        // });
        // let backend = RpcClient(transport);
        // let meta = backend.metadata().await?;
        // let response = sube::query(&backend, &meta, "system/account/0x1234", None).await?;
        // ```
        //
        // Here we demonstrate with a dummy transport to show the API:

        let transport = HttpTransport::new("https://kreivo.io", |url: &str, body: Vec<u8>| {
            let url = url.to_string();
            async move {
                // In a real embedded scenario this would be your network stack.
                let _ = (url, body);
                Err(sube::Error::ChainUnavailable)
            }
        });

        let mut backend = RpcClient(transport);

        // This will fail with ChainUnavailable since our dummy transport doesn't connect.
        // Replace the closure body with a real HTTP POST to make it work.
        match backend.metadata().await {
            Ok(meta) => println!("Got metadata with {} pallets", meta.pallets.len()),
            Err(e) => println!("Expected error (dummy transport): {e}"),
        }

        Ok(())
    })
}
