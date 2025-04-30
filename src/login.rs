use std::time::SystemTime;

use axum::extract::{Form, Query};
use axum::http::{StatusCode, Uri, header::SET_COOKIE};
use axum::response::{Html, IntoResponse};
use form_urlencoded::byte_serialize as encode_uri;
use jsonwebtoken::{Header, encode};
use serde::Deserialize;

use crate::auth::{Claims, KEYS};
use crate::config::CONFIG;
use crate::errors::ServerError;
use crate::html::redirect::{HtmlRedirectConfig, gen_html_redirect};

pub const ROUTE_PATH: &str = "/login";

#[derive(Deserialize)]
pub struct LoginQuery {
    redirect: Option<String>,
}

pub async fn get(query: Query<LoginQuery>) -> impl IntoResponse {
    let form_action = if let Some(redirect) = &query.redirect {
        format!(
            "/login?redirect={}",
            encode_uri(redirect.as_bytes()).collect::<String>()
        )
    } else {
        "/login".to_string()
    };

    Html(format!(
        r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Login | Welcome Back</title>
<link rel="stylesheet" href="/_assets/reset.css">
<link rel="stylesheet" href="/_assets/login.css">
</head>
<body>
<main class="login-container">
	<div class="login-card">
		<div class="login-decoration">
			<div class="circles">
				<div class="circle circle-1"></div>
				<div class="circle circle-2"></div>
				<div class="circle circle-3"></div>
			</div>
			<h2 class="welcome-message">Welcome<br>Back!</h2>
		</div>

		<div class="login-content">
			<header>
				<h1>Sign In</h1>
				<p>Please login to access your account</p>
			</header>

			<form class="login-form" action="{form_action}" method="post">
				<div class="form-field">
					<label for="username">Username</label>
					<div class="input-wrapper">
						<input type="text" id="username" name="username" placeholder="yourusername" autofocus  required>
						<span class="input-icon">👤</span>
					</div>
				</div>

				<div class="form-field">
					<label for="password">Password</label>
					<div class="input-wrapper">
						<input type="password" id="password" name="password" placeholder="••••••••" required>
						<span class="input-icon">🔒</span>
					</div>
				</div>

				<button type="submit" class="login-button">Sign In</button>
			</form>
		</div>
	</div>
</main>
</body>
</html>"###,
    ))
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
            Html(gen_html_redirect(HtmlRedirectConfig {
                title: "Login Successful",
                url: query.redirect.as_deref().unwrap_or("/"),
                success: true,
                message: format!(
                    "Welcome back, {}! You'll be redirected to your dashboard.",
                    claims.sub,
                )
                .as_str(),
                ..Default::default()
            })),
        )
            .into_response())
    } else {
        Ok((
            StatusCode::UNAUTHORIZED,
            Html(gen_html_redirect(HtmlRedirectConfig {
                title: "Login Failed",
                url: uri
                    .path_and_query()
                    .map(|p_and_q| p_and_q.as_str())
                    .unwrap_or("/"),
                success: false,
                message: "Incorrect username or password. You'll be redirected to the login page.",
                ..Default::default()
            })),
        )
            .into_response())
    }
}
