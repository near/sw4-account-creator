use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use near_jsonrpc_client::{
    errors::JsonRpcError,
    methods::status::{RpcStatusError, RpcStatusRequest},
    JsonRpcClient,
};
use near_primitives::hash::CryptoHash;

/// Fetches the current block hash from the NEAR RPC node
pub(crate) async fn current_block_hash(
    near_rpc: &JsonRpcClient,
) -> Result<CryptoHash, JsonRpcError<RpcStatusError>> {
    tracing::debug!("Fetching current block hash from NEAR RPC node...");
    near_rpc
        .call(RpcStatusRequest)
        .await
        .map(|status| status.sync_info.latest_block_hash)
}

/// Constantly updates the block hash in the given `Arc<RwLock<CryptoHash>>` every 30 seconds
/// by fetching the latest block hash from the NEAR RPC node
/// This is used to ensure that the block hash used in the transaction is always up to date
pub(crate) async fn update_block_hash(
    near_rpc: JsonRpcClient,
    block_hash: Arc<RwLock<CryptoHash>>,
) {
    loop {
        tokio::time::sleep(Duration::from_secs(30)).await;
        tracing::debug!("Updating block hash...");
        let current = match current_block_hash(&near_rpc).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("failed to fetch current block hash: {:?}", e);
                continue;
            }
        };
        let mut b = block_hash.write().unwrap();
        *b = current;
    }
}
