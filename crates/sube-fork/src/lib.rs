//! An in-process fork of a live Substrate parachain for local development.
//!
//! Like chopsticks, but as a sube backend: download a full-state [`Snapshot`]
//! once, then boot a [`Fork`] from it offline and build blocks with the real
//! runtime, run by smoldot's executor. The relay chain is mocked, so no relay
//! chain, collators or zombienet are needed.
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use std::{cell::RefCell, rc::Rc};
//! use sube_fork::{Fork, Snapshot};
//!
//! let fork = Rc::new(RefCell::new(Fork::new(Snapshot::load("kreivo.snap")?)?));
//! let mut sube = sube_fork::connect(fork.clone()).await?;
//! // Query and submit as usual; each submission builds a block.
//! sube.query("timestamp/now").await?;
//! // The next block lands an hour later.
//! fork.borrow_mut().time_travel(600);
//! fork.borrow_mut().new_block()?;
//! # Ok(()) }
//! ```
//!
//! Everything is single-threaded and poll-driven: nothing is spawned.

mod error;
mod executor;
mod fork;
mod header;
mod para;
mod rpc;
mod snapshot;
mod storage;
mod trie;
mod util;

use std::cell::RefCell;
use std::rc::Rc;

pub use error::{Error, Result};
pub use executor::{RuntimeInfo, SignaturePolicy};
pub use fork::{Block, Built, Fork, Rejected, STATE_HISTORY};
pub use rpc::ForkRpc;
pub use snapshot::{Progress, Snapshot};
pub use storage::{Base, Diff, Entries, View};

pub type Hash = [u8; 32];

/// The sube client a fork serves.
pub type ForkSube = sube::Sube<sube::rpc::chainhead::ChainHead<ForkRpc>>;

/// Open a sube session on a shared fork, with its metadata loaded.
pub async fn connect(fork: Rc<RefCell<Fork>>) -> sube::Result<ForkSube> {
    use sube::Backend as _;
    let mut chain = sube::rpc::chainhead::ChainHead::new(ForkRpc::new(fork)).await?;
    let metadata = chain.metadata().await?;
    Ok(sube::Sube::from_parts(chain, Rc::new(metadata)))
}
