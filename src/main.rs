use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use actix_files as fs;
use actix_web::{error, web, App, HttpResponse, HttpServer, Responder, Result};
use anyhow::Context as AnyhowContext;
use clap::Parser;
use dotenv::dotenv;
use near_account_id::AccountId;
use near_crypto::{InMemorySigner, PublicKey, Signer};
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::action::{Action, AddKeyAction, CreateAccountAction, TransferAction};
use near_primitives::transaction::{SignedTransaction, Transaction};
use near_primitives::types::{BlockReference, Finality};
use near_primitives::views::FinalExecutionStatus;
use near_primitives_core::account::AccessKey;
use near_primitives_core::hash::CryptoHash;
use near_primitives_core::types::{Balance, Nonce};
use serde::Deserialize;
use tera::{Context, Tera};
use tracing_subscriber::EnvFilter;

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
}

/// Structure for the form data from the index page
/// We accept Strings from the user and need to validate the data later
#[derive(Deserialize)]
pub struct FormData {
    account_id: String,
    public_key: String,
}

/// Data shared between the actix-web handlers
/// This is used to store the base signer, the nonce, the block hash, the NEAR RPC client and the funding amount
/// Available as `near` (`web::Data`) in the actix-web handlers
#[derive(Clone)]
struct NearData {
    base_signer: InMemorySigner,
    nonce: Arc<Mutex<Nonce>>,
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
    let data = form.into_inner();

    let block_hash = *near.block_hash.read().unwrap();
    // for now we keep the lock while calling send_create_account(),
    // but TODO is to not do that and just retry if the nonce fails
    let mut nonce = near.nonce.lock().unwrap();
    *nonce += 1;

    match send_create_account(
        &near.rpc,
        &near.base_signer,
        &data.account_id,
        &data.public_key,
        *nonce,
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
    nonce: Nonce,
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

    let tx = Transaction {
        signer_id: base_signer.account_id.clone(),
        public_key: base_signer.public_key.clone(),
        nonce,
        receiver_id: new_account,
        block_hash,
        actions,
    };
    let (hash, _size) = tx.get_hash_and_size();
    let sig = base_signer.sign(hash.as_ref());
    let signed_transaction = SignedTransaction::new(sig, tx);

    tracing::debug!("Sending transaction to NEAR RPC node...");
    // TODO: retry on nonce error
    match near_rpc
        .call(methods::broadcast_tx_commit::RpcBroadcastTxCommitRequest { signed_transaction })
        .await
    {
        Ok(r) => {
            if matches!(r.status, FinalExecutionStatus::SuccessValue(_)) {
                tracing::info!("transaction execution succeeded: {:?}", &r.status);
                Ok(())
            } else {
                tracing::warn!("transaction execution failed: {:?}", &r.status);
                Err(anyhow::anyhow!(
                    "transaction execution failed: {:?}",
                    &r.status
                ))
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Fetches the current block hash from the NEAR RPC node
async fn current_block_hash(near_rpc: &JsonRpcClient) -> std::io::Result<CryptoHash> {
    tracing::debug!("Fetching current block hash from NEAR RPC node...");
    match near_rpc.call(methods::status::RpcStatusRequest).await {
        Ok(status) => Ok(status.sync_info.latest_block_hash),
        Err(e) => Err(std::io::Error::other(e)),
    }
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
            QueryResponseKind::AccessKey(a) => Arc::new(Mutex::new(a.nonce)),
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
    let block_hash = Arc::new(RwLock::new(current_block_hash(&rpc).await?));

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
    // TODO: CORS to deny requests from other domains
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(tera.clone()))
            .app_data(web::Data::new(near_data.clone()))
            .service(fs::Files::new("/assets", "assets").show_files_listing()) // for serving the static files
            .route("/", web::get().to(index))
            .route("/create_account", web::post().to(create_account))
    })
    .bind(format!("0.0.0.0:{:0>5}", args.server_port))?
    .run()
    .await?;

    Ok(())
}
