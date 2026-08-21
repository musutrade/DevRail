//! 权限 Handler：权限组树

use crate::auth::RequirePermission;
use crate::error::ApiError;
use crate::models::PermissionGroupResponse;
use crate::permissions::PermissionRead;
use crate::services;
use crate::AppState;
use axum::extract::State;
use axum::Json;

pub async fn groups(
    State(state): State<AppState>,
    _auth: RequirePermission<PermissionRead>,
) -> Result<Json<Vec<PermissionGroupResponse>>, ApiError> {
    services::permissions::groups(&state.pool).await.map(Json)
}
