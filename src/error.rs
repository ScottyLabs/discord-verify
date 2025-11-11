use axum::response::{IntoResponse, Redirect, Response};

#[derive(Debug)]
pub enum AppError {
    VerificationExpired,
    WrongDiscordAccount,
    AlreadyLinkedToDifferentAccount,
    DiscordNotLinked,
    KeycloakError(anyhow::Error),
    RedisError(anyhow::Error),
    InternalError(anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            AppError::VerificationExpired => Redirect::to("/error?msg=expired").into_response(),
            AppError::WrongDiscordAccount => {
                Redirect::to("/error?msg=wrong_account").into_response()
            }
            AppError::AlreadyLinkedToDifferentAccount => {
                Redirect::to("/error?msg=already_linked").into_response()
            }
            AppError::DiscordNotLinked => Redirect::to("/error?msg=not_linked").into_response(),
            AppError::KeycloakError(e) => {
                tracing::error!("Keycloak error: {:?}", e);
                Redirect::to("/error?msg=server_error").into_response()
            }
            AppError::RedisError(e) => {
                tracing::error!("Redis error: {:?}", e);
                Redirect::to("/error?msg=server_error").into_response()
            }
            AppError::InternalError(e) => {
                tracing::error!("Internal error: {:?}", e);
                Redirect::to("/error?msg=server_error").into_response()
            }
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(err: anyhow::Error) -> Self {
        AppError::InternalError(err)
    }
}

impl From<redis::RedisError> for AppError {
    fn from(err: redis::RedisError) -> Self {
        AppError::RedisError(err.into())
    }
}
