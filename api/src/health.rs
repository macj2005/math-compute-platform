use axum::{Json, extract::State};
use serde::Serialize;

use crate::api_error::ApiError;
use crate::app_state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    status: String,
}

pub async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

pub async fn ready_check(State(state): State<AppState>) -> Result<Json<HealthResponse>, ApiError> {
    sqlx::query("SELECT 1")
        .execute(&state.db_pool)
        .await
        .map_err(|error| {
            tracing::error!(%error, "database readiness check failed");
            ApiError::service_unavailable("database is not ready")
        })?;

    Ok(Json(HealthResponse {
        status: "ready".to_string(),
    }))
}
