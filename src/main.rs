use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::http::{StatusCode, Uri};
use axum::{Router, response::Html, routing::get, serve};

mod assets;
mod auth;
mod config;
mod errors;
mod files;
mod hello_world;
mod html;
mod index;
mod login;
mod upload;

#[tokio::main]
async fn main() -> Result<()> {
    let app = Router::new()
        .route(assets::ROUTE_PATH, get(assets::get))
        .route(hello_world::ROUTE_PATH, get(hello_world::get))
        .route(login::ROUTE_PATH, get(login::get).post(login::post))
        .route(index::ROUTE_PATH, get(index::get))
        .route(files::ROUTE_PATH_ROOT, get(files::root_get))
        .route(
            files::ROUTE_PATH_ROOT_EMPTY,
            get(files::root_get)
                .post(files::root_post)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            files::ROUTE_PATH,
            get(files::get)
                .post(files::post)
                .layer(DefaultBodyLimit::disable()),
        )
        .route(
            upload::ROUTE_PATH,
            get(upload::get)
                .post(upload::post)
                .put(upload::put)
                .layer(DefaultBodyLimit::disable()),
        )
        .fallback(async move |uri: Uri| {
            (
                StatusCode::NOT_FOUND,
                Html(html::error::not_found(
                    uri.path_and_query().map(|pq| pq.as_str()),
                )),
            )
        });

    let listener = tokio::net::TcpListener::bind(config::CONFIG.listen).await?;

    serve(listener, app).await?;

    Ok(())
}
