use std::time::SystemTime;

use axum::extract::{Form, Query};
use axum::http::{StatusCode, Uri, header::SET_COOKIE};
use axum::response::{Html, IntoResponse};
use form_urlencoded::byte_serialize as encode_uri;
use jsonwebtoken::{Header, encode};
use sailfish::TemplateOnce;
use serde::Deserialize;

use crate::auth::{Claims, KEYS};
use crate::config::CONFIG;
use crate::errors::ServerError;
use crate::templates;

pub const ROUTE_PATH: &str = "/login";

#[derive(Deserialize)]
pub struct LoginQuery {
    redirect: Option<String>,
}

pub async fn get(query: Query<LoginQuery>) -> Result<impl IntoResponse, ServerError> {
    let login_templates = templates::Login {
        url: ROUTE_PATH,
        redirect: query.redirect.as_deref(),
    };
    Ok(Html(login_templates.render_once()?))
}

#[derive(Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub async fn post(
    query: Query<LoginQuery>,
    uri: Uri,
    Form(login_form): Form<LoginForm>,
) -> Result<impl IntoResponse, ServerError> {
    if login_form.username == CONFIG.username && login_form.password == CONFIG.password {
        let claims = Claims {
            sub: login_form.username,
            exp: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + CONFIG.token_expiry,
        };
        let token = encode(&Header::default(), &claims, &KEYS.encoding).map_err(|_| {
            ServerError::TokenCreation {
                redirect_uri: query.redirect.clone(),
            }
        })?;

        Ok((
            StatusCode::OK,
            [(
                SET_COOKIE,
                format!(
                    "access_token={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
                    token, CONFIG.token_expiry
                ),
            )],
            Html(
                templates::Redirect {
                    title: "Login Successful",
                    url: query.redirect.as_deref().unwrap_or("/"),
                    success: true,
                    message: format!(
                        "Welcome back, {}! You'll be redirected to your dashboard.",
                        claims.sub,
                    )
                    .as_str(),
                    ..Default::default()
                }
                .render_once()?,
            ),
        )
            .into_response())
    } else {
        Ok((
            StatusCode::UNAUTHORIZED,
            Html(templates::Redirect {
                title: "Login Failed",
                url: uri
                    .path_and_query()
                    .map(|p_and_q| p_and_q.as_str())
                    .unwrap_or("/"),
                success: false,
                message: "Incorrect username or password. You'll be redirected to the login page.",
                ..Default::default()
            }.render_once()?),
        )
            .into_response())
    }
}
