use std::str::FromStr;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

use actix_files as fs;
use actix_web::{error, web, App, HttpResponse, HttpServer, Responder, Result};
use anyhow::Context as _;
use clap::Parser;
use dotenv::dotenv;
use near_account_id::AccountId;
use near_crypto::InMemorySigner;
use near_jsonrpc_client::{methods, JsonRpcClient};
use near_jsonrpc_primitives::types::query::QueryResponseKind;
use near_primitives::types::{BlockReference, Finality};
use near_primitives_core::hash::CryptoHash;
use near_primitives_core::types::Balance;
use serde::Deserialize;
use tera::{Context, Tera};
use tracing_subscriber::EnvFilter;

#[cfg(feature = "contract-helper")]
mod contract_helper;
mod create_account;
mod utils;

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
        let account_id =
            if account_id.ends_with(format!(".{}", base_signer_account_id).to_string().as_str()) {
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
pub(crate) struct NearData {
    pub(crate) base_signer: InMemorySigner,
    pub(crate) nonce: Arc<AtomicU64>,
    pub(crate) block_hash: Arc<RwLock<CryptoHash>>,
    pub(crate) rpc: JsonRpcClient,
    pub(crate) funding_amount: Balance,
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

    match create_account::send_create_account(
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
        utils::block_hash::current_block_hash(&rpc)
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

    tokio::spawn(async move {
        utils::block_hash::update_block_hash(rpc.clone(), block_hash.clone()).await
    });

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
