//! 仪表盘 Handler：统计指标

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::DashboardStats;
use crate::permissions::DashboardRead;
use crate::services;
use crate::AppState;
use axum::extract::State;
use axum::Json;

pub async fn stats(
    State(state): State<AppState>,
    auth: RequirePermission<DashboardRead>,
) -> Result<Json<DashboardStats>, ApiError> {
    services::permissions::stats(&state.pool, &auth)
        .await
        .map(Json)
}
