use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use axum::extract::multipart::MultipartError;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Json, Response};
use rustix::io::Errno;
use serde_json::json;
use tokio::task::JoinError;

use crate::html;

pub enum ServerError {
    MissingCredentials { current_uri: String },
    TokenCreation { redirect_uri: Option<String> },
    InvalidToken { current_uri: String },
    IoError(IoError),
    InternalError(String),
    InvalidUploadForm,
    FileAlreadyExists,
    FileIsNotCreated,
    UploadMetaIsBroken,
    MissingContentLength,
    InvalidContentLength,
    MissingChunkIndex,
    InvalidChunkIndex,
    ChunkIsOngoing,
    ChunkIsCompleted,
    UploadChunkFailed,
    Custom { status: StatusCode, message: String },
}
impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        match self {
            Self::MissingCredentials { current_uri } => (
                StatusCode::UNAUTHORIZED,
                Html(html::redirect::gen_html_redirect(
                    html::redirect::HtmlRedirectConfig {
                        success: false,
                        url: format!("/login?redirect={current_uri}").as_str(),
                        title: "Missing Credentials",
                        message: "It seems you havan't login yet. Please login first.",
                        ..Default::default()
                    },
                )),
            )
                .into_response(),

            Self::TokenCreation { redirect_uri } => {
                if let Some(redirect_uri) = redirect_uri {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Html(html::redirect::gen_html_redirect(
                            html::redirect::HtmlRedirectConfig {
                                success: false,
                                url: format!("/login?redirect={redirect_uri}").as_str(),
                                title: "Token Creation Error",
                                message: "Failed to create access token. Please try login again.",
                                ..Default::default()
                            },
                        )),
                    )
                        .into_response()
                } else {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Html(html::redirect::gen_html_redirect(
                            html::redirect::HtmlRedirectConfig {
                                success: false,
                                url: "/login",
                                title: "Token Creation Error",
                                message: "Failed to create access token. Please try login again.",
                                ..Default::default()
                            },
                        )),
                    )
                        .into_response()
                }
            }

            Self::InvalidToken { current_uri } => (
                StatusCode::UNAUTHORIZED,
                Html(html::redirect::gen_html_redirect(
                    html::redirect::HtmlRedirectConfig {
                        success: false,
                        url: format!("/login?redirect={current_uri}").as_str(),
                        title: "Invalid Token",
                        message: "The access token is invalid. Please login again.",
                        ..Default::default()
                    },
                )),
            )
                .into_response(),

            Self::IoError(io_error) => match io_error.kind() {
                IoErrorKind::NotFound => (
                    StatusCode::NOT_FOUND,
                    Html(html::error::not_found::<String>(None)),
                )
                    .into_response(),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Html(html::error::internal_server_error(io_error.to_string())),
                )
                    .into_response(),
            },

            Self::InternalError(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(html::error::internal_server_error(message)),
            )
                .into_response(),

            Self::InvalidUploadForm => (
                StatusCode::BAD_REQUEST,
                Html(html::error::bad_request("Invalid upload form.")),
            )
                .into_response(),

            Self::FileAlreadyExists => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "The file already exists."})),
            )
                .into_response(),

            Self::FileIsNotCreated => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "The file is not created."})),
            )
                .into_response(),

            Self::UploadMetaIsBroken => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "The upload file meta information is broken."})),
            )
                .into_response(),

            Self::MissingContentLength => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing content length in the HTTP header."})),
            )
                .into_response(),

            Self::InvalidContentLength => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "The content length in the HTTP header is invalid."})),
            )
                .into_response(),

            Self::MissingChunkIndex => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing chunk index in the HTTP header."})),
            )
                .into_response(),

            Self::InvalidChunkIndex => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "The chunk index in the HTTP header is invalid."})),
            )
                .into_response(),

            Self::ChunkIsOngoing => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "The chunk is ongoing."})),
            )
                .into_response(),

            Self::ChunkIsCompleted => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "The chunk is completed."})),
            )
                .into_response(),

            Self::UploadChunkFailed => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Failed to upload the chunk."})),
            )
                .into_response(),

            Self::Custom { status, message } => (
                status,
                Html(html::error::gen_html_error(html::error::HtmlErrorConfig {
                    status,
                    message: message.as_str(),
                    ..Default::default()
                })),
            )
                .into_response(),
        }
    }
}
impl From<IoError> for ServerError {
    fn from(io_error: IoError) -> Self {
        ServerError::IoError(io_error)
    }
}
impl From<Errno> for ServerError {
    fn from(errno: Errno) -> Self {
        ServerError::IoError(errno.into())
    }
}
impl From<JoinError> for ServerError {
    fn from(join_error: JoinError) -> Self {
        ServerError::InternalError(join_error.to_string())
    }
}
impl From<MultipartError> for ServerError {
    fn from(multipart_error: MultipartError) -> Self {
        ServerError::Custom {
            status: multipart_error.status(),
            message: multipart_error.body_text(),
        }
    }
}
