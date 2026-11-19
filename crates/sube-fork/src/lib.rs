//! An in-process fork of a live Substrate parachain for local development.
//!
//! Like chopsticks, but as a library: download a full-state [`Snapshot`]
//! once, then boot a [`Fork`] from it offline and build blocks with the real
//! runtime, run by smoldot's executor. The relay chain is mocked, so no relay
//! chain, collators or zombienet are needed.
//!
//! Clients talk to a fork through [`ForkRpc`], a JSON-RPC session. With its
//! `fork` feature, sube uses it as a backend: `Sube::connect("fork:kreivo.snap")`.
//!
//! ```no_run
//! # fn demo() -> Result<(), sube_fork::Error> {
//! use sube_fork::{Fork, Snapshot};
//!
//! let mut fork = Fork::new(Snapshot::load("kreivo.snap")?)?;
//! // The next block lands an hour later.
//! fork.time_travel(600);
//! let block = fork.new_block()?;
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

pub use error::{Error, Result};
pub use executor::{RuntimeInfo, SignaturePolicy};
pub use fork::{Block, Built, Fork, Rejected, STATE_HISTORY};
pub use rpc::{ForkRpc, RpcError};
pub use snapshot::{Progress, Snapshot, Transport};
pub use storage::{Base, Diff, Entries, View};

pub type Hash = [u8; 32];

/// A fork shared by its JSON-RPC sessions and the code driving it.
pub type SharedFork = std::rc::Rc<std::cell::RefCell<Fork>>;
