use std::{
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::Context;
use near_account_id::AccountId;
use near_crypto::{InMemorySigner, PublicKey, Signer};
use near_jsonrpc_client::{
    errors::{JsonRpcError, JsonRpcServerError},
    methods::{self, tx::RpcTransactionError},
    JsonRpcClient,
};
use near_primitives::{
    account::AccessKey,
    action::{Action, AddKeyAction, CreateAccountAction, TransferAction},
    errors::{InvalidTxError, TxExecutionError},
    hash::CryptoHash,
    transaction::{SignedTransaction, Transaction},
    types::Balance,
    views::FinalExecutionStatus,
};

use crate::utils::nonce::retry_nonce;

// TODO: rate limit or somehow gate this faucet

/// Creates a Transaction with actions:
/// - CreateAccount
/// - AddKey
/// - Transfer (funding the account)
/// Signs the transaction by the base signer and sends it to the NEAR RPC node
pub(crate) async fn send_create_account(
    near_rpc: &JsonRpcClient,
    base_signer: &InMemorySigner,
    account_id: &str,
    public_key: &str,
    nonce: &AtomicU64,
    block_hash: CryptoHash,
    funding_amount: Balance,
) -> anyhow::Result<()> {
    tracing::debug!(
        "Creating account {} with public key {}",
        account_id,
        public_key
    );
    let new_account = AccountId::from_str(account_id)
        .with_context(|| format!("failed parsing account ID: {}", account_id))?;
    let pkey = PublicKey::from_str(public_key)
        .with_context(|| format!("failed parsing public key: {}", public_key))?;

    let actions = vec![
        Action::CreateAccount(CreateAccountAction {}),
        Action::AddKey(Box::new(AddKeyAction {
            public_key: pkey,
            access_key: AccessKey::full_access(),
        })),
        Action::Transfer(TransferAction {
            deposit: funding_amount,
        }),
    ];
    let mut next_nonce = nonce.fetch_add(1, Ordering::SeqCst) + 1;

    loop {
        let tx = Transaction {
            signer_id: base_signer.account_id.clone(),
            public_key: base_signer.public_key.clone(),
            nonce: next_nonce,
            receiver_id: new_account.clone(),
            block_hash,
            actions: actions.clone(),
        };
        let (hash, _size) = tx.get_hash_and_size();
        let sig = base_signer.sign(hash.as_ref());
        let signed_transaction = SignedTransaction::new(sig, tx.clone());

        tracing::debug!(
            "Sending transaction creating {} with nonce {} to NEAR RPC node...",
            account_id,
            next_nonce
        );
        match near_rpc
            .call(methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest { signed_transaction })
            .await
        {
            Ok(r) => match r.status {
                FinalExecutionStatus::SuccessValue(_) => {
                    tracing::info!(
                        "transaction execution succeeded for {}: {:?}",
                        account_id,
                        &r.status
                    );
                    return Ok(());
                }
                // looks like this one doesn't show up, and instead we get an Err(JsonRpcError) in this case,
                // but might as well handle this case here too
                FinalExecutionStatus::Failure(TxExecutionError::InvalidTxError(
                    InvalidTxError::InvalidNonce { tx_nonce, ak_nonce },
                )) => {
                    next_nonce = retry_nonce(nonce, next_nonce, tx_nonce, ak_nonce);
                    tracing::debug!(
                        "retrying creating {} with nonce {} after nonce {} was rejected with current access key nonce {}",
                        account_id,
                        next_nonce,
                        tx_nonce,
                        ak_nonce,
                    );
                }
                _ => {
                    tracing::warn!("transaction execution failed: {:?}", &r.status);
                    return Err(anyhow::anyhow!(
                        "transaction execution failed: {:?}",
                        &r.status
                    ));
                }
            },
            Err(JsonRpcError::ServerError(JsonRpcServerError::HandlerError(
                RpcTransactionError::InvalidTransaction {
                    context: InvalidTxError::InvalidNonce { tx_nonce, ak_nonce },
                },
            ))) => {
                next_nonce = retry_nonce(nonce, next_nonce, tx_nonce, ak_nonce);
                tracing::debug!(
                    "retrying creating {} with nonce {} after nonce {} was rejected with current access key nonce {}",
                    account_id,
                    next_nonce,
                    tx_nonce,
                    ak_nonce,
                );
            }
            Err(e) => return Err(e.into()),
        };
    }
}
