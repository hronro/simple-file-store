use axum::response::Html;
use sailfish::TemplateOnce;

use crate::auth::Claims;
use crate::errors::ServerError;
use crate::files;
use crate::templates::Home;

pub const ROUTE_PATH: &str = "/";

pub async fn get(claims: Option<Claims>) -> Result<Html<String>, ServerError> {
    let home_tempalte = Home {
        claims,
        jump_url: files::ROUTE_PATH_ROOT,
    };

    Ok(Html(home_tempalte.render_once()?))
}
