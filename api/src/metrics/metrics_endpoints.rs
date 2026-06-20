use axum::{Json, extract::State};

use crate::api_error::ApiError;
use crate::app_state::AppState;
use crate::metrics::{get_metrics_from_db, metrics_types::MetricsResponse};

pub async fn get_metrics(State(state): State<AppState>) -> Result<Json<MetricsResponse>, ApiError> {
    let metrics = get_metrics_from_db(&state.db_pool).await.map_err(|error| {
        tracing::error!(%error, "failed to get metrics from Postgres");
        ApiError::internal("failed to get metrics")
    })?;

    Ok(Json(metrics))
}
