use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use actix_files as fs;
use actix_web::{error, web, App, HttpResponse, HttpServer, Responder, Result};
use anyhow::Context as AnyhowContext;
use clap::Parser;
use dotenv::dotenv;
use near_account_id::AccountId;
use near_crypto::{InMemorySigner, PublicKey, Signer};
use near_jsonrpc_client::errors::{JsonRpcError, JsonRpcServerError};
use near_jsonrpc_client::methods::status::{RpcStatusError, RpcStatusRequest};
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_jsonrpc_primitives::types::transactions::RpcTransactionError;
use near_primitives::action::{Action, AddKeyAction, CreateAccountAction, TransferAction};
use near_primitives::errors::{InvalidTxError, TxExecutionError};
use near_primitives::transaction::{SignedTransaction, Transaction};
use near_primitives::types::{BlockReference, Finality};
use near_primitives::views::FinalExecutionStatus;
use near_primitives_core::account::AccessKey;
use near_primitives_core::hash::CryptoHash;
use near_primitives_core::types::{Balance, Nonce};
use serde::Deserialize;
use tera::{Context, Tera};
use tracing_subscriber::EnvFilter;

#[cfg(feature = "contract-helper")]
mod contract_helper;

// ======== STRUCTURES ========

/// CLI arguments for the service
#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Port to listen on, default 10000
    #[clap(short, long, env, default_value_t = 10000)]
    server_port: u16,
    /// NEAR RPC URL to send transactions to
    #[clap(long, env)]
    near_rpc_url: String,
    /// Signer AccountId
    #[clap(long, env)]
    base_signer_account_id: String,
    /// Signer SecretKey
    #[clap(long, env)]
    base_signer_secret_key: String,
    /// Amount to fund new accounts with, default 100 NEAR
    #[clap(long, env, default_value_t = 100_000_000_000_000_000_000_000_000)]
    funding_amount: Balance,
    #[cfg(feature = "contract-helper")]
    /// ExplorerDB connection string to fetch the data for contract-helper feature
    #[clap(long, env)]
    database_url: String,
}

/// Structure for the form data from the index page
/// We accept Strings from the user and need to validate the data later
#[derive(Deserialize)]
pub struct FormData {
    account_id: String,
    public_key: String,
}

impl FormData {
    /// Normalizes the form data by trimming whitespace from the strings
    fn normalize(self, base_signer_account_id: &str) -> Self {
        let account_id = self.account_id.trim().to_string();
        // If account_id provided by the user does not end with .statelessnet, we add it
        let account_id = if account_id.ends_with(".statelessnet") {
            account_id
        } else {
            format!("{}.{}", account_id, base_signer_account_id)
        };
        FormData {
            account_id,
            public_key: self.public_key.trim().to_string(),
        }
    }
}

/// Data shared between the actix-web handlers
/// This is used to store the base signer, the nonce, the block hash, the NEAR RPC client and the funding amount
/// Available as `near` (`web::Data`) in the actix-web handlers
#[derive(Clone)]
struct NearData {
    base_signer: InMemorySigner,
    nonce: Arc<AtomicU64>,
    block_hash: Arc<RwLock<CryptoHash>>,
    rpc: JsonRpcClient,
    funding_amount: Balance,
}

// ======== ENDPOINTS ========

/// Endpoint: /
/// Index page repsonding with just a template rendering
/// The template has a form for submission that should be handled by the method `create_account`
async fn index(tera: web::Data<Tera>) -> Result<impl Responder> {
    tracing::debug!("GET /");
    let _context = Context::new();

    let rendered = tera.render("index.html.tera", &_context).map_err(|err| {
        error::ErrorInternalServerError(format!("Failed to render template: {:?}", err))
    })?;

    Ok(HttpResponse::Ok().content_type("text/html").body(rendered))
}

/// Endpoint: /create_account
/// Handles the form submission from the index page
/// Validates the form data and sends a transaction to create the account
/// Responds with a success or error message (HTML)
async fn create_account(
    near: web::Data<NearData>,
    tera: web::Data<Tera>,
    form: web::Form<FormData>,
) -> Result<impl Responder> {
    tracing::debug!("POST /create_account");
    // Normalization happens here, we don't validate the account_id for the validity of the NEAR account id
    // we expect the validation to happen during the parsing of the form data in `send_create_account()` function
    let data = form
        .into_inner()
        .normalize(near.base_signer.account_id.as_str());

    let block_hash = *near.block_hash.read().unwrap();

    match send_create_account(
        &near.rpc,
        &near.base_signer,
        &data.account_id,
        &data.public_key,
        near.nonce.as_ref(),
        block_hash,
        near.funding_amount,
    )
    .await
    {
        Ok(_) => {
            tracing::info!(
                "successfully created {} {}",
                &data.account_id,
                &data.public_key
            );

            let mut context = Context::new();
            context.insert("account_id", &data.account_id);
            context.insert("public_key", &data.public_key);

            match tera.render("form_success.html.tera", &context) {
                Ok(rendered) => Ok(HttpResponse::Ok().content_type("text/html").body(rendered)),
                Err(err) => Err(error::ErrorInternalServerError(format!(
                    "Failed to render template: {:?}",
                    err
                ))),
            }
        }
        Err(err) => {
            tracing::warn!("Failed to create account: {:?}", err);
            let mut context = Context::new();
            context.insert("error_message", format!("{:?}", err).as_str());

            match tera.render("form_fail.html.tera", &context) {
                Ok(rendered) => Ok(HttpResponse::Ok().content_type("text/html").body(rendered)),
                Err(err) => Err(error::ErrorInternalServerError(format!(
                    "Failed to render template: {:?}",
                    err
                ))),
            }
        }
    }
}

/// Returns a nonce greater than both the nonces we know are too small.
fn new_nonce(nonce1: Nonce, nonce2: Nonce) -> Nonce {
    std::cmp::max(nonce1, nonce2) + 1
}

/// Returns and stores in `nonce` a new nonce to try with after getting an InvalidNonce{ tx_nonce, ak_nonce } error
fn retry_nonce(nonce: &AtomicU64, old_nonce: Nonce, tx_nonce: Nonce, ak_nonce: Nonce) -> Nonce {
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

// ======== PRIVATE FUNCTIONS ========

// TODO: rate limit or somehow gate this faucet

/// Creates a Transaction with actions:
/// - CreateAccount
/// - AddKey
/// - Transfer (funding the account)
/// Signs the transaction by the base signer and sends it to the NEAR RPC node
async fn send_create_account(
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

/// Fetches the current block hash from the NEAR RPC node
async fn current_block_hash(
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
async fn update_block_hash(near_rpc: JsonRpcClient, block_hash: Arc<RwLock<CryptoHash>>) {
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    tracing::info!(
        "Starting {}:{}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    );

    let args = Args::parse();
    let tera = Tera::new("templates/**/*").unwrap();

    #[cfg(feature = "contract-helper")]
    let pool = sqlx::PgPool::connect(&args.database_url).await?;

    tracing::debug!("Parsing base signer account ID and secret key...");
    let base_signer = InMemorySigner::from_secret_key(
        AccountId::from_str(&args.base_signer_account_id)?,
        near_crypto::SecretKey::from_str(&args.base_signer_secret_key)?,
    );

    tracing::debug!("Establishing connection to NEAR RPC node...");
    let rpc = JsonRpcClient::connect(&args.near_rpc_url);
    let nonce = match rpc
        .call(methods::query::RpcQueryRequest {
            block_reference: BlockReference::Finality(Finality::None),
            request: near_primitives::views::QueryRequest::ViewAccessKey {
                account_id: base_signer.account_id.clone(),
                public_key: base_signer.public_key.clone(),
            },
        })
        .await
    {
        Ok(r) => match r.kind {
            QueryResponseKind::AccessKey(a) => Arc::new(AtomicU64::new(a.nonce)),
            _ => anyhow::bail!(
                "received unexpected query response when getting access key info: {:?}",
                r.kind
            ),
        },
        Err(e) => {
            anyhow::bail!(
                "failed fetching access key info for {} {}: {:?}",
                &base_signer.account_id,
                &base_signer.public_key,
                e,
            );
        }
    };
    let block_hash = Arc::new(RwLock::new(
        current_block_hash(&rpc)
            .await
            .context("failed fetching latest block hash")?,
    ));

    tracing::debug!("Spawning the block hash updater...");

    let near_data = NearData {
        base_signer,
        nonce,
        block_hash: block_hash.clone(),
        rpc: rpc.clone(),
        funding_amount: args.funding_amount,
    };

    tokio::spawn(async move { update_block_hash(rpc.clone(), block_hash.clone()).await });

    tracing::info!("Starting the HTTP server on port {}...", args.server_port);

    HttpServer::new(move || {
        #[allow(unused_mut)]
        let mut app = App::new()
            .wrap(actix_cors::Cors::permissive())
            .app_data(web::Data::new(tera.clone()))
            .app_data(web::Data::new(near_data.clone()))
            .service(fs::Files::new("/assets", "assets").show_files_listing()) // for serving the static files
            .route("/", web::get().to(index))
            .route("/create_account", web::post().to(create_account));

        #[cfg(feature = "contract-helper")]
        {
            app = app
                .app_data(web::Data::new(pool.clone()))
                .service(contract_helper::account_scope());
        }

        app
    })
    .bind(format!("0.0.0.0:{:0>5}", args.server_port))?
    .run()
    .await?;

    Ok(())
}
