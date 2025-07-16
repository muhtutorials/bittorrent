use anyhow::Error;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

#[derive(Debug)]
pub struct ErrResp {
    pub status: StatusCode,
    pub error: Vec<u8>,
}

#[derive(Serialize)]
struct ErrMsg {
    reason: String
}

impl ErrResp {
    pub fn new(status: StatusCode, err: Error) -> Self {
        let error = ErrMsg { reason: err.to_string() };
        let error = serde_bencode::to_bytes(&error)
            .expect("failed to bencode error message");
        Self { status, error }
    }

    pub fn bad_request(err: Error) -> Self {
        Self::new(StatusCode::BAD_REQUEST, err)
    }

    pub fn server_error(err: Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, err)
    }
}

impl IntoResponse for ErrResp {
    fn into_response(self) -> Response {
        (self.status, self.error).into_response()
    }
}