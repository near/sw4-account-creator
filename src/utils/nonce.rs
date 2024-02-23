use std::sync::atomic::{AtomicU64, Ordering};

use near_primitives::types::Nonce;

/// Returns a nonce greater than both the nonces we know are too small.
fn new_nonce(nonce1: Nonce, nonce2: Nonce) -> Nonce {
    std::cmp::max(nonce1, nonce2) + 1
}

/// Returns and stores in `nonce` a new nonce to try with after getting an InvalidNonce{ tx_nonce, ak_nonce } error
pub(crate) fn retry_nonce(
    nonce: &AtomicU64,
    old_nonce: Nonce,
    tx_nonce: Nonce,
    ak_nonce: Nonce,
) -> Nonce {
    if tx_nonce != old_nonce {
        tracing::warn!(
            "NEAR RPC node reported that our transaction's nonce was {}, when we remember sending {}",
            tx_nonce, old_nonce
        );
    }
    let prev_nonce = nonce
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            Some(new_nonce(n, ak_nonce))
        })
        .unwrap();
    // now we call new_nonce() again because fetch_update() returns the old value
    new_nonce(prev_nonce, ak_nonce)
}
