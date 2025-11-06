use crate::{error::AppError, state::AppState};
use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_oidc::{EmptyAdditionalClaims, OidcClaims};
use serde::Deserialize;
use std::sync::Arc;
use tower_sessions::Session;

#[derive(Deserialize)]
pub struct VerifyQuery {
    state: String,
}

#[axum::debug_handler]
pub async fn verify_start(
    State(state): State<Arc<AppState>>,
    Query(query): Query<VerifyQuery>,
    oidc_claims: OidcClaims<EmptyAdditionalClaims>,
    session: Session,
) -> Response {
    tracing::info!("verify_start called with state token: {}", query.state);

    let state_token = query.state;
    let user_id = oidc_claims.subject().to_string();
    tracing::info!("User authenticated: {}", user_id);

    // Get verification data
    let verification = {
        let verifications = state.pending_verifications.read().await;
        verifications.get(&state_token).cloned()
    };

    tracing::debug!(
        "Verification data lookup result: {:?}",
        verification.is_some()
    );

    let verification = match verification {
        Some(v) => v,
        None => {
            tracing::warn!(
                "Verification expired or not found for state: {}",
                state_token
            );
            return AppError::VerificationExpired.into_response();
        }
    };

    tracing::info!(
        "Checking Keycloak federated identities for user: {}",
        user_id
    );

    // Check if Discord already linked
    let identities = match state.keycloak.get_federated_identities(&user_id).await {
        Ok(i) => {
            tracing::info!("Found {} federated identities", i.len());
            i
        }
        Err(e) => {
            tracing::error!("Failed to get federated identities: {:?}", e);
            return AppError::KeycloakError(e).into_response();
        }
    };

    if let Some(discord) = identities
        .iter()
        .find(|i| i.identity_provider.as_deref() == Some("discord"))
    {
        tracing::info!("Discord already linked, validating Discord user ID");
        // Already linked, validate
        if discord.user_id.as_deref() == Some(&verification.discord_user_id.to_string()) {
            tracing::info!("Discord ID matches, completing verification");

            // Send verification completion event to bot
            let completion = crate::state::VerificationComplete {
                discord_user_id: verification.discord_user_id,
                guild_id: verification.guild_id,
                keycloak_user_id: user_id.clone(),
            };

            if let Err(e) = state.verification_tx.send(completion) {
                tracing::error!("Failed to send verification completion event: {}", e);
            }

            // Clean up
            state
                .pending_verifications
                .write()
                .await
                .remove(&state_token);

            tracing::info!("Redirecting to success page");
            return Redirect::to(&format!("/success?state={}", state_token)).into_response();
        } else {
            // Linked to different Discord account
            tracing::warn!(
                "Discord account mismatch. Expected: {}, Got: {:?}",
                verification.discord_user_id,
                discord.user_id
            );
            return AppError::AlreadyLinkedToDifferentAccount.into_response();
        }
    }

    // Need to link Discord, trigger re-authentication with Discord IdP
    tracing::info!("No Discord linked yet, initiating OIDC flow with Discord IdP hint");

    // Store the state_token in session so we can retrieve it after Discord linking
    if let Err(e) = session
        .insert("pending_verification_state", &state_token)
        .await
    {
        tracing::error!("Failed to store state in session: {}", e);
        return AppError::InternalError(anyhow::anyhow!("Session error")).into_response();
    }

    // Redirect to /link-callback which will complete the verification after Discord is linked
    let redirect_uri = format!("{}/link-callback", state.config.app_url);
    let linking_url = format!(
        "{}/realms/{}/protocol/openid-connect/auth?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email%20profile&kc_action=idp_link:discord",
        state.config.keycloak_url,
        state.config.keycloak_realm,
        urlencoding::encode(&state.config.keycloak_oidc_client_id),
        urlencoding::encode(&redirect_uri),
    );

    Redirect::to(&linking_url).into_response()
}

#[axum::debug_handler]
pub async fn link_callback(
    State(state): State<Arc<AppState>>,
    oidc_claims: Option<OidcClaims<EmptyAdditionalClaims>>,
    session: Session,
) -> Response {
    tracing::info!("link_callback called");

    let Some(claims) = oidc_claims else {
        return AppError::InternalError(anyhow::anyhow!("User not authenticated")).into_response();
    };

    let user_id = claims.subject().to_string();
    tracing::info!("User authenticated: {}", user_id);

    // Retrieve state_token from session
    let state_token: String = match session.get("pending_verification_state").await {
        Ok(Some(token)) => token,
        Ok(None) => {
            tracing::warn!("No pending verification state found in session");
            return AppError::VerificationExpired.into_response();
        }
        Err(e) => {
            tracing::error!("Failed to retrieve state from session: {}", e);
            return AppError::InternalError(anyhow::anyhow!("Session error")).into_response();
        }
    };

    tracing::info!("Retrieved state_token from session: {}", state_token);

    // Clean up session
    if let Err(e) = session.remove::<String>("pending_verification_state").await {
        tracing::warn!("Failed to remove state from session: {}", e);
    }

    // Get verification data
    let verification = {
        let verifications = state.pending_verifications.read().await;
        verifications.get(&state_token).cloned()
    };

    let verification = match verification {
        Some(v) => v,
        None => {
            tracing::warn!(
                "Verification expired or not found for state: {}",
                state_token
            );
            return AppError::VerificationExpired.into_response();
        }
    };

    // Verify Discord was linked correctly
    let identities = match state.keycloak.get_federated_identities(&user_id).await {
        Ok(i) => {
            tracing::info!("Found {} federated identities after Discord auth", i.len());
            for identity in &i {
                tracing::debug!(
                    "Identity provider: {:?}, user_id: {:?}",
                    identity.identity_provider,
                    identity.user_id
                );
            }
            i
        }
        Err(e) => {
            tracing::error!("Failed to get federated identities: {:?}", e);
            return AppError::KeycloakError(e).into_response();
        }
    };

    let discord_identity = match identities
        .iter()
        .find(|i| i.identity_provider.as_deref() == Some("discord"))
    {
        Some(i) => {
            tracing::info!("Found Discord identity with user_id: {:?}", i.user_id);
            i
        }
        None => {
            tracing::warn!("Discord identity not found after auth flow. User may have cancelled.");
            return AppError::DiscordNotLinked.into_response();
        }
    };

    if discord_identity.user_id.as_deref() != Some(&verification.discord_user_id.to_string()) {
        // Wrong account, unlink it
        let _ = state
            .keycloak
            .delete_federated_identity(&user_id, "discord")
            .await;
        return AppError::WrongDiscordAccount.into_response();
    }

    // Send verification completion event to bot
    let completion = crate::state::VerificationComplete {
        discord_user_id: verification.discord_user_id,
        guild_id: verification.guild_id,
        keycloak_user_id: user_id.clone(),
    };

    if let Err(e) = state.verification_tx.send(completion) {
        tracing::error!("Failed to send verification completion event: {}", e);
        // Continue anyway, user verified but role assignment will fail
    }

    // Success, clean up and redirect
    state
        .pending_verifications
        .write()
        .await
        .remove(&state_token);

    Redirect::to(&format!("/success?state={}", state_token)).into_response()
}
