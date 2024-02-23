use actix_web::web;

use account_activity::account_activity_handler;
use account_by_public_key::account_by_public_key_handler;
use account_create::account_create_handler;
use account_likely_nfts::account_likely_nfts_handler;
use account_likely_tokens::account_likely_tokens_handler;

mod account_activity;
mod account_by_public_key;
mod account_create;
mod account_likely_nfts;
mod account_likely_tokens;

// Function to create and return the accounts scope
pub fn account_scope() -> actix_web::Scope {
    web::scope("/account")
        // .route("/create", web::get().to(create_account_handler))
        .route(
            "/keys/{public_key}",
            web::get().to(account_by_public_key_handler),
        )
        .route(
            "/{account_id}/likelyTokensFromBlock",
            web::get().to(account_likely_tokens_handler),
        )
        .route(
            "/{account_id}/txns",
            web::get().to(account_activity_handler),
        )
        .route(
            "/{account_id}/likelyNFTsFromBlock",
            web::get().to(account_likely_nfts_handler),
        )
        .route("/create", web::post().to(account_create_handler))
}

// Define the accounts scope as a public constant
// pub const ACCOUNT_SCOPE: actix_web::Scope = web::scope("/accounts")
//     // .route("/create", web::get().to(create_account_handler))
//     .route(
//         "/keys/{public_key}",
//         web::get().to(account_by_public_key_handler),
//     );
// .route(
//     "/{account_id}/txns",
//     web::get().to(account_activity_handler),
// )
// .route(
//     "/{account_id}/likelyTokensFromBlock",
//     web::put().to(account_likely_tokens_handler),
// )
// .route(
//     "/{account_id}/likelyNFTsFromBlock",
//     web::delete().to(account_likely_nfts_handler),
// );
