use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct AccountCreateResponse {
    result: Option<AccountInfo>,
    error: Option<AccountCreateError>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct AccountInfo {
    account_id: String,
    public_key: String,
}

impl AccountInfo {
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
        AccountInfo {
            account_id,
            public_key: self.public_key.trim().to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct AccountCreateError {
    message: String,
}

pub(crate) async fn account_create_handler(
    data: web::Data<crate::NearData>,
    account_info: web::Json<AccountInfo>,
) -> impl Responder {
    // Extract the account_id and public_key from the request body
    let normalized_account_info = account_info
        .clone()
        .normalize(&data.base_signer.account_id.as_str());
    let account_id = normalized_account_info.account_id.clone();
    let public_key = normalized_account_info.public_key.clone();

    // Call the send_account_create function from crate::create_account
    let result = crate::create_account::send_create_account(
        &data.rpc,
        &data.base_signer,
        &account_id,
        &public_key,
        data.nonce.as_ref(),
        *data.block_hash.read().unwrap(),
        data.funding_amount,
    )
    .await;

    // Return an appropriate response based on the result
    match result {
        Ok(_) => {
            let response = AccountCreateResponse {
                result: Some(AccountInfo {
                    account_id: account_id.clone(),
                    public_key: public_key.clone(),
                }),
                error: None,
            };
            HttpResponse::Ok().json(response)
        }
        Err(err) => {
            let response = AccountCreateResponse {
                result: None,
                error: Some(AccountCreateError {
                    message: err.to_string(),
                }),
            };
            HttpResponse::InternalServerError().json(response)
        }
    }
}
