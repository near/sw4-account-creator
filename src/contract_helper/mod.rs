use actix_web::web;

use account_by_public_key::account_by_public_key_handler;

mod account_by_public_key;

// Function to create and return the accounts scope
pub fn account_scope() -> actix_web::Scope {
    web::scope("/accounts")
        // .route("/create", web::get().to(create_account_handler))
        .route(
            "/keys/{public_key}",
            web::get().to(account_by_public_key_handler),
        )
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
