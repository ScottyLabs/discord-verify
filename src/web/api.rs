use crate::{error::AppError, state::AppState};
use axum::{
    Json,
    extract::{Path, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
pub struct VerifyStatusResponse {
    pub status: String,
    pub discord_username: Option<String>,
}

#[axum::debug_handler]
pub async fn verify_status(
    State(state): State<Arc<AppState>>,
    Path(state_token): Path<String>,
) -> Result<Json<VerifyStatusResponse>, AppError> {
    // Check if verification exists
    let verification = {
        let verifications = state.pending_verifications.read().await;
        verifications.get(&state_token).cloned()
    };

    match verification {
        Some(v) => Ok(Json(VerifyStatusResponse {
            status: "pending".to_string(),
            discord_username: Some(v.discord_username),
        })),
        None => Ok(Json(VerifyStatusResponse {
            status: "not_found".to_string(),
            discord_username: None,
        })),
    }
}

/// Health check endpoint
#[axum::debug_handler]
pub async fn health() -> impl IntoResponse {
    "OK"
}
