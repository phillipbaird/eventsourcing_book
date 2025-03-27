use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use disintegrate::DecisionError;

use crate::domain::cart::CartError;

#[derive(Debug)]
pub enum ClientError {
    Decision(DecisionError<CartError>),
    Domain(CartError),
    Payload(String),
    Internal(anyhow::Error),
}

impl IntoResponse for ClientError {
    fn into_response(self) -> Response {
        #[derive(serde::Serialize)]
        struct ErrorResponse {
            message: String,
        }

        let (status, message) = match self {
            ClientError::Decision(decision_error) => match decision_error {
                DecisionError::EventStore(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "EventStore problem. Please ask your system administrator to check the logs."
                        .to_owned(),
                ),
                DecisionError::StateStore(_) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "StateStore problem. Please ask your system administrator to check the logs."
                        .to_owned(),
                ),
                DecisionError::Domain(cart_error) => {
                    (StatusCode::BAD_REQUEST, cart_error.to_string())
                }
            },
            ClientError::Domain(cart_error) => (StatusCode::BAD_REQUEST, cart_error.to_string()),
            ClientError::Payload(message) => (StatusCode::BAD_REQUEST, message),
            ClientError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Please ask your system administrator to check the logs.".to_owned(),
            ),
        };

        (status, Json(ErrorResponse { message })).into_response()
    }
}

impl From<DecisionError<CartError>> for ClientError {
    fn from(decision_error: DecisionError<CartError>) -> Self {
        ClientError::Decision(decision_error)
    }
}

impl From<CartError> for ClientError {
    fn from(cart_error: CartError) -> Self {
        ClientError::Domain(cart_error)
    }
}

impl From<anyhow::Error> for ClientError {
    fn from(value: anyhow::Error) -> Self {
        ClientError::Internal(value)
    }
}
