use actix_web::{web, HttpResponse, Responder, Result};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct AccountLikelyTokensQuery {
    #[serde(alias = "fromBlockTimestamp")]
    from_block_timestamp: u64,
}

pub(crate) async fn account_likely_tokens_handler(
    pool: web::Data<PgPool>,
    account_id: web::Path<String>,
    query_params: web::Query<AccountLikelyTokensQuery>,
) -> Result<impl Responder> {
    let account_id = account_id.into_inner();
    let from_block_timestamp = query_params.into_inner().from_block_timestamp;
    tracing::debug!(
        "account_likely_tokens_handler called. account_id: {:?}, from_block_timestamp: {:?}",
        account_id,
        from_block_timestamp
    );

    // convert from_block_timestamp to BigDecimal
    let from_block_timestamp = sqlx::types::BigDecimal::from(from_block_timestamp);

    let result: Option<serde_json::Value> = sqlx::query_scalar!(
        r#"
        WITH last_block AS (
            SELECT block_timestamp
            FROM blocks
            ORDER BY block_timestamp DESC
            LIMIT 1
        ),
        received AS (
            SELECT DISTINCT receipt_receiver_account_id
            FROM action_receipt_actions
            WHERE args->'args_json'->>'receiver_id' = $1
                AND action_kind = 'FUNCTION_CALL'
                AND args->>'args_json' IS NOT NULL
                AND args->>'method_name' IN ('ft_transfer', 'ft_transfer_call', 'ft_mint')
                AND receipt_included_in_block_timestamp <= (SELECT block_timestamp FROM last_block)
                AND receipt_included_in_block_timestamp > $2
        ),
        called_by_user AS (
            SELECT DISTINCT receipt_receiver_account_id
            FROM action_receipt_actions
            WHERE receipt_predecessor_account_id = $1
                AND action_kind = 'FUNCTION_CALL'
                AND (args->>'method_name' LIKE 'ft_%' OR args->>'method_name' = 'storage_deposit')
                AND receipt_included_in_block_timestamp <= (SELECT block_timestamp FROM last_block)
                AND receipt_included_in_block_timestamp > $2
        )
        SELECT json_build_object(
            'lastBlockTimestamp', (SELECT block_timestamp FROM last_block)::text,
            'list', (SELECT array_agg(receipt_receiver_account_id) FROM (
                SELECT receipt_receiver_account_id FROM received
                UNION
                SELECT receipt_receiver_account_id FROM called_by_user
            ) AS combined_results),
            'version', '1.0.0'
        ) FROM last_block;
        "#,
        account_id,
        from_block_timestamp,
    )
    .fetch_optional(&**pool)
    .await
    .map_err(|e| {
        tracing::warn!("Failed to execute query: {:?}", e);
        HttpResponse::InternalServerError().finish()
    })
    .unwrap_or_default()
    .unwrap_or_default();

    match result {
        Some(json) => Ok(HttpResponse::Ok().json(json)),
        None => Ok(HttpResponse::Ok().json(serde_json::json!({"message": "No data found"}))),
    }
}
