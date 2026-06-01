use axum::response::Html;
use sailfish::TemplateOnce;

use crate::auth::Claims;
use crate::files;
use crate::templates::Home;

pub const ROUTE_PATH: &str = "/";

pub async fn get(claims: Option<Claims>) -> Html<String> {
    let home_template = Home {
        claims,
        jump_url: files::ROUTE_PATH_ROOT,
    };

    Html(
        home_template
            .render_once()
            .expect("home template render is infallible with default String buffer"),
    )
}
