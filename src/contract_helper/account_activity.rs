use actix_web::{web, HttpResponse, Responder, Result};
use serde::Deserialize;
use sqlx::PgPool;

// Define a struct to receive the query parameters
#[derive(Deserialize)]
pub(crate) struct AccountActivityQuery {
    order: Option<String>,
    page: Option<i64>,
    per_page: Option<i64>,
}

pub(crate) async fn account_activity_handler(
    pool: web::Data<PgPool>,
    account_id: web::Path<String>,
    web::Query(query_params): web::Query<AccountActivityQuery>,
) -> Result<impl Responder> {
    // Set default values if None
    let order = query_params.order.unwrap_or_else(|| "desc".to_string());
    let page = query_params.page.unwrap_or(1);
    let per_page = query_params.per_page.unwrap_or(10);

    // Calculate offset for pagination
    let offset = (page - 1) * per_page;

    // Ensure 'order' is either 'asc' or 'desc'
    let order = if order.to_lowercase() == "asc" {
        "ASC"
    } else {
        "DESC"
    };

    let result: Option<serde_json::Value> = sqlx::query_scalar(
        &format!( // Use format! to interpolate the 'order' and pagination variables
            r#"
            SELECT json_build_object(
                'txns', json_agg(
                    json_build_object(
                        'receipt_id', r.receipt_id,
                        'predecessor_account_id', r.predecessor_account_id,
                        'receiver_account_id', r.receiver_account_id,
                        'transaction_hash', t.transaction_hash,
                        'included_in_block_hash', b.block_hash,
                        'block_timestamp', r.included_in_block_timestamp,
                        'block', json_build_object(
                            'block_height', b.block_height
                        ),
                        'actions', (SELECT json_agg(json_build_object('action', a.action_kind, 'method', a.args->>'method'))
                                    FROM transaction_actions a
                                    WHERE a.transaction_hash = t.transaction_hash),
                        'actions_agg', (SELECT json_build_object('deposit', SUM((a.args->>'deposit')::numeric))
                                        FROM transaction_actions a
                                        WHERE a.transaction_hash = t.transaction_hash),
                        'outcomes', (SELECT json_build_object('status', o.status)
                                    FROM execution_outcomes o
                                    WHERE o.receipt_id = t.transaction_hash),
                        'outcomes_agg', (SELECT json_build_object('transaction_fee', SUM(o.tokens_burnt))
                                        FROM execution_outcomes o
                                        WHERE o.receipt_id = t.transaction_hash),
                        'logs', '[]'::json
                    ) ORDER BY b.block_height {}
                )
            )
            FROM transactions t
            JOIN receipts r ON t.converted_into_receipt_id = r.receipt_id
            JOIN blocks b ON t.included_in_block_hash = b.block_hash
            WHERE r.predecessor_account_id = $1 OR r.receiver_account_id = $1
                OFFSET {}
                LIMIT {}
            "#, order, offset, per_page
        )
    )
    .bind(&account_id.to_owned())
    .fetch_optional(&**pool)
    .await
    .map_err(|e| {
        tracing::warn!("Failed to execute query: {:?}", e);
        HttpResponse::InternalServerError().finish()
    }).unwrap_or_default();

    match result {
        Some(json) => Ok(HttpResponse::Ok().json(json)),
        None => Ok(HttpResponse::Ok().json(serde_json::json!({"txns": []}))),
    }
}
