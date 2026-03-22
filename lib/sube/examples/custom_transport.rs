//! Using sube with a custom WebSocket transport on embedded targets.
//!
//! This shows how the `edge-ws` backend enables sube on ESP32, embassy,
//! and other no_std targets. Any `embedded_io_async::{Read, Write}` stream
//! can be used — TCP, TLS, or a custom transport.
//!
//! On real hardware, replace the connection setup with your platform's
//! TCP/TLS stack (e.g. `esp-idf-svc`, `embassy-net`).
//!
//! ```rust,ignore
//! // ESP32 example (conceptual):
//! let tcp = TcpSocket::connect("kreivo.io:443").await?;
//! let tls = TlsStream::new(tcp, "kreivo.io").await?;
//! let ws = sube::rpc::edge::Backend::connect(tls, "kreivo.io", "/").await?;
//! let mut chain = sube::rpc::chainhead::ChainHead::new(ws).await?;
//! let meta = chain.metadata().await?;
//! ```

fn main() {
    println!("This example demonstrates the edge-ws API for embedded targets.");
    println!("See the doc comment above for the conceptual usage pattern.");
    println!();
    println!("To run on real hardware, build with:");
    println!("  --features ws-edge --target xtensa-esp32s3-espidf");
}
