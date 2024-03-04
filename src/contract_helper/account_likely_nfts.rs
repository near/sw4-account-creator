use actix_web::{web, HttpResponse, Responder, Result};
use serde::Deserialize;
use sqlx::PgPool;

#[derive(Deserialize)]
pub struct AccountLikelyNftsQuery {
    #[serde(alias = "fromBlockTimestamp")]
    from_block_timestamp: i64,
}

pub(crate) async fn account_likely_nfts_handler(
    pool: web::Data<PgPool>,
    account_id: web::Path<String>,
    query_params: web::Query<AccountLikelyNftsQuery>,
) -> Result<impl Responder> {
    let account_id = account_id.into_inner();
    let from_block_timestamp = query_params.into_inner().from_block_timestamp;
    tracing::debug!(
        "account_likely_nfts_handler called. account_id: {:?}, from_block_timestamp: {:?}",
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
        ownership_change_function_calls AS (
            SELECT DISTINCT receipt_receiver_account_id AS account_id
            FROM action_receipt_actions
            WHERE args->'args_json'->>'receiver_id' = $1
                AND action_kind = 'FUNCTION_CALL'
                AND args->>'args_json' IS NOT NULL
                AND args->>'method_name' LIKE 'nft_%'
                AND receipt_included_in_block_timestamp <= (SELECT block_timestamp FROM last_block)
                AND receipt_included_in_block_timestamp > $2
        ),
        ownership_change_events AS (
            SELECT DISTINCT emitted_by_contract_account_id AS account_id
            FROM assets__non_fungible_token_events
            WHERE token_new_owner_account_id = $1
                AND emitted_at_block_timestamp <= (SELECT block_timestamp FROM last_block)
                AND emitted_at_block_timestamp > $2
        )
        SELECT json_build_object(
            'lastBlockTimestamp', (SELECT block_timestamp FROM last_block)::text,
            'list', (SELECT array_agg(account_id) FROM (
                SELECT account_id FROM ownership_change_function_calls
                UNION
                SELECT account_id FROM ownership_change_events
            ) AS combined_results),
            'version', '1.0.0'
        ) FROM last_block;
        "#,
        account_id,
        from_block_timestamp
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
