//! Public subscription types for watching storage and tracking transactions.

use crate::prelude::*;
use crate::{Response, Result};

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

/// Transaction submission result with lifecycle tracking.
pub struct TxProgress {
    pub(crate) rx: futures_channel::mpsc::UnboundedReceiver<TxStatus>,
}

impl TxProgress {
    /// Get the next transaction status event.
    pub async fn next(&mut self) -> Option<TxStatus> {
        use futures_util::StreamExt;
        self.rx.next().await
    }

    /// Wait until the transaction is included in a best block.
    pub async fn wait_included(&mut self) -> Result<TxInBlock> {
        while let Some(status) = self.next().await {
            match status {
                TxStatus::InBestBlock { block, index } | TxStatus::Finalized { block, index } => {
                    return Ok(TxInBlock { block, index });
                }
                TxStatus::Invalid { error }
                | TxStatus::Dropped { error }
                | TxStatus::Error { error } => {
                    return Err(crate::Error::Node(error));
                }
                _ => continue,
            }
        }
        Err(crate::Error::SubscriptionClosed)
    }

    /// Wait until the transaction is finalized.
    pub async fn wait_finalized(&mut self) -> Result<TxInBlock> {
        while let Some(status) = self.next().await {
            match status {
                TxStatus::Finalized { block, index } => {
                    return Ok(TxInBlock { block, index });
                }
                TxStatus::Invalid { error }
                | TxStatus::Dropped { error }
                | TxStatus::Error { error } => {
                    return Err(crate::Error::Node(error));
                }
                _ => continue,
            }
        }
        Err(crate::Error::SubscriptionClosed)
    }
}

/// A transaction that has been included in a block.
#[derive(Debug, Clone)]
pub struct TxInBlock {
    pub block: [u8; 32],
    pub index: u32,
}

/// Watch storage value changes across finalized blocks.
pub struct StorageWatch {
    pub(crate) rx: futures_channel::mpsc::UnboundedReceiver<Result<Response>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_channel::mpsc;

    fn make_progress() -> (mpsc::UnboundedSender<TxStatus>, TxProgress) {
        let (tx, rx) = mpsc::unbounded();
        (tx, TxProgress { rx })
    }

    #[async_std::test]
    async fn next_returns_items_from_channel() {
        let (tx, mut progress) = make_progress();
        tx.unbounded_send(TxStatus::Validated).unwrap();
        tx.unbounded_send(TxStatus::Broadcasted { num_peers: 3 })
            .unwrap();
        drop(tx);

        match progress.next().await.unwrap() {
            TxStatus::Validated => {}
            other => panic!("expected Validated, got {:?}", other),
        }
        match progress.next().await.unwrap() {
            TxStatus::Broadcasted { num_peers } => assert_eq!(num_peers, 3),
            other => panic!("expected Broadcasted, got {:?}", other),
        }
        assert!(progress.next().await.is_none());
    }

    #[async_std::test]
    async fn wait_included_with_finalized() {
        let (tx, mut progress) = make_progress();
        let block = [0xaa; 32];
        tx.unbounded_send(TxStatus::Finalized { block, index: 5 })
            .unwrap();
        drop(tx);

        let result = progress.wait_included().await.unwrap();
        assert_eq!(result.block, block);
        assert_eq!(result.index, 5);
    }

    #[async_std::test]
    async fn wait_included_with_invalid_returns_error() {
        let (tx, mut progress) = make_progress();
        tx.unbounded_send(TxStatus::Invalid {
            error: "bad tx".into(),
        })
        .unwrap();
        drop(tx);

        let err = progress.wait_included().await.unwrap_err();
        match err {
            crate::Error::Node(msg) => assert_eq!(msg, "bad tx"),
            other => panic!("expected Node error, got {:?}", other),
        }
    }

    #[async_std::test]
    async fn wait_finalized_skips_non_terminal() {
        let (tx, mut progress) = make_progress();
        let block = [0xbb; 32];
        tx.unbounded_send(TxStatus::Validated).unwrap();
        tx.unbounded_send(TxStatus::Broadcasted { num_peers: 1 })
            .unwrap();
        tx.unbounded_send(TxStatus::InBestBlock {
            block: [0xcc; 32],
            index: 0,
        })
        .unwrap();
        tx.unbounded_send(TxStatus::Finalized { block, index: 7 })
            .unwrap();
        drop(tx);

        let result = progress.wait_finalized().await.unwrap();
        assert_eq!(result.block, block);
        assert_eq!(result.index, 7);
    }

    #[async_std::test]
    async fn channel_dropped_returns_subscription_closed() {
        let (tx, mut progress) = make_progress();
        tx.unbounded_send(TxStatus::Validated).unwrap();
        drop(tx);

        let err = progress.wait_finalized().await.unwrap_err();
        match err {
            crate::Error::SubscriptionClosed => {}
            other => panic!("expected SubscriptionClosed, got {:?}", other),
        }
    }
}

impl StorageWatch {
    /// Get the next storage value update.
    pub async fn next(&mut self) -> Option<Result<Response>> {
        use futures_util::StreamExt;
        self.rx.next().await
    }
}
