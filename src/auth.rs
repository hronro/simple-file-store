use std::sync::LazyLock;

use axum::RequestPartsExt;
use axum::extract::{FromRequestParts, OptionalFromRequestParts};
use axum::http::request::Parts;
use axum_extra::{
    TypedHeader,
    headers::{Authorization, Cookie, authorization::Bearer},
};
use form_urlencoded::byte_serialize as encode_uri;
use jsonwebtoken::{DecodingKey, EncodingKey, Validation, decode};
use serde::{Deserialize, Serialize};

use crate::config::CONFIG;
use crate::errors::ServerError;

pub static KEYS: LazyLock<Keys> = LazyLock::new(|| Keys::new(CONFIG.secret.as_bytes()));

pub struct Keys {
    pub encoding: EncodingKey,
    pub decoding: DecodingKey,
}
impl Keys {
    fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: u64,
}
impl<S> FromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = ServerError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Extract the token from the authorization header or cookie,
        // header has higher priority.

        if let Ok(TypedHeader(Authorization(bearer))) =
            parts.extract::<TypedHeader<Authorization<Bearer>>>().await
        {
            let token_data =
                decode::<Claims>(bearer.token(), &KEYS.decoding, &Validation::default()).map_err(
                    |_| ServerError::InvalidToken {
                        current_uri: parts
                            .uri
                            .path_and_query()
                            .map(|p_and_q| encode_uri(p_and_q.as_str().as_bytes()).collect())
                            .unwrap_or("/".to_string()),
                    },
                )?;

            return Ok(token_data.claims);
        }

        if let Ok(TypedHeader(cookie)) = parts.extract::<TypedHeader<Cookie>>().await
            && let Some(token) = cookie.get("access_token")
        {
            let token_data = decode::<Claims>(token, &KEYS.decoding, &Validation::default())
                .map_err(|_| ServerError::InvalidToken {
                    current_uri: parts
                        .uri
                        .path_and_query()
                        .map(|p_and_q| encode_uri(p_and_q.as_str().as_bytes()).collect())
                        .unwrap_or("/".to_string()),
                })?;

            return Ok(token_data.claims);
        }

        Err(ServerError::MissingCredentials {
            current_uri: parts
                .uri
                .path_and_query()
                .map(|p_and_q| encode_uri(p_and_q.as_str().as_bytes()).collect())
                .unwrap_or("/".to_string()),
        })
    }
}
impl<S> OptionalFromRequestParts<S> for Claims
where
    S: Send + Sync,
{
    type Rejection = ServerError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        match <Claims as FromRequestParts<_>>::from_request_parts(parts, state).await {
            Err(ServerError::MissingCredentials { .. }) => Ok(None),

            Err(e) => Err(e),

            Ok(claims) => Ok(Some(claims)),
        }
    }
}
