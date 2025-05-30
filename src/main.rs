use anyhow::Result;
use axum::extract::{DefaultBodyLimit, Request};
use axum::http::{StatusCode, Uri};
use axum::{Router, response::Html, routing::get, serve};
use hyper::body::Incoming;
use hyper::service::service_fn as hyper_service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo};
use tokio::spawn;
use tokio_rustls::TlsAcceptor;
use tower_service::Service;

mod assets;
mod auth;
mod config;
mod errors;
mod files;
mod hello_world;
mod home;
mod html;
mod login;
mod templates;
mod upload;

#[tokio::main]
async fn main() -> Result<()> {
    let app = Router::new()
        .route(assets::ROUTE_PATH, get(assets::get))
        .route(hello_world::ROUTE_PATH, get(hello_world::get))
        .route(login::ROUTE_PATH, get(login::get).post(login::post))
        .route(home::ROUTE_PATH, get(home::get))
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

    if let Some(ref tls) = config::CONFIG.tls {
        let tls_acceptor = TlsAcceptor::from(tls.clone());

        loop {
            let app = app.clone();
            let tls_acceptor = tls_acceptor.clone();

            if let Ok((tcp_stream, _addr)) = listener.accept().await {
                spawn(async move {
                    let Ok(stream) = tls_acceptor.accept(tcp_stream).await else {
                        return;
                    };

                    let stream = TokioIo::new(stream);

                    let hyper_service = hyper_service_fn(move |request: Request<Incoming>| {
                        app.clone().call(request)
                    });

                    let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection_with_upgrades(stream, hyper_service)
                        .await;
                });
            };
        }
    } else {
        serve(listener, app).await?;
    }

    Ok(())
}
