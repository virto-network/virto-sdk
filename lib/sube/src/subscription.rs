//! Public subscription types for watching storage and tracking transactions.
//!
//! These are placeholder types for the future subscription API.

use crate::prelude::*;

/// Transaction lifecycle status.
#[derive(Debug, Clone)]
pub enum TxStatus {
    Validated,
    Broadcasted { num_peers: u32 },
    InBestBlock { block: [u8; 32], index: u32 },
    Finalized { block: [u8; 32], index: u32 },
    Invalid { error: String },
    Dropped { error: String },
    Error { error: String },
}

/// A transaction that has been included in a block.
#[derive(Debug, Clone)]
pub struct TxInBlock {
    pub block: [u8; 32],
    pub index: u32,
}
