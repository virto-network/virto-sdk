//! Download a verified full-state snapshot of a live chain, then boot a fork
//! from it with `Sube::connect("fork:<file>")`.
//!
//! Run with: cargo run -p sube --example fork_snapshot --features fork,wss --release -- wss://kreivo.io kreivo.snap

use sube::fork::{Progress, Snapshot};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let url = args.next().unwrap_or_else(|| "wss://kreivo.io".into());
    let out = args.next().unwrap_or_else(|| "fork.snap".into());
    smol::block_on(async {
        let mut node = sube::rpc::ws::Backend::new(&url).await?;
        let start = std::time::Instant::now();
        let snapshot = Snapshot::download(&mut node, None, |p| match p {
            Progress::Keys(n) if n % 50_000 == 0 => eprintln!("  {n} keys"),
            Progress::Values { done, total } if done == total => eprintln!("  {total} values"),
            Progress::ChildTries { done, total } if done == total => {
                eprintln!("  {total} child tries")
            }
            _ => {}
        })
        .await?;
        snapshot.save(&out)?;
        eprintln!(
            "saved {} #{} ({} keys) to {out} in {:.1?}",
            snapshot.chain_name,
            snapshot.number().unwrap_or_default(),
            snapshot.state.main.len(),
            start.elapsed()
        );
        Ok(())
    })
}
