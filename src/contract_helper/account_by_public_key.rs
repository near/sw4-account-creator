use actix_web::{web, HttpResponse, Responder, Result};
use sqlx::PgPool;

pub(crate) async fn account_by_public_key_handler(
    pool: web::Data<PgPool>,
    public_key: web::Path<String>,
) -> Result<impl Responder> {
    let public_key = public_key.into_inner();

    let result: Option<serde_json::Value> = sqlx::query_scalar(
        r#"
        SELECT json_build_object(
            'keys', json_agg(
                json_build_object(
                    'public_key', ak.public_key,
                    'account_id', acc.account_id,
                    'permission_kind', ak.permission_kind,
                    'created', json_build_object(
                        'transaction_hash', cr.transaction_hash,
                        'block_timestamp', TO_CHAR(TO_TIMESTAMP(cr.block_timestamp / 1000000000)::timestamp at time zone 'UTC', 'YYYY-MM-DD HH24:MI:SS.US')
                    ),
                    'deleted', json_build_object(
                        'transaction_hash', dl.transaction_hash,
                        'block_timestamp', TO_CHAR(TO_TIMESTAMP(dl.block_timestamp / 1000000000)::timestamp at time zone 'UTC', 'YYYY-MM-DD HH24:MI:SS.US')
                    )
                )
            )
        )
        FROM access_keys ak
        LEFT JOIN accounts acc ON ak.account_id = acc.account_id
        LEFT JOIN transactions cr ON ak.created_by_receipt_id = cr.converted_into_receipt_id
        LEFT JOIN transactions dl ON ak.deleted_by_receipt_id = dl.converted_into_receipt_id
        WHERE ak.public_key = $1
        "#,
    )
    .bind(public_key)
    .fetch_optional(&**pool)
    .await
    .map_err(|e| {
        tracing::warn!("Failed to execute query: {:?}", e);
        HttpResponse::InternalServerError().finish()
    }).unwrap_or_default();

    match result {
        Some(json) => Ok(HttpResponse::Ok().json(json)),
        None => Ok(HttpResponse::Ok().json(serde_json::json!({"keys": "[]"}))),
    }
}
