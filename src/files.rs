use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use axum::body::Body;
use axum::extract::{Multipart, OriginalUri, Path};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::{Html, IntoResponse};
use futures::{TryStreamExt, pin_mut};
use sailfish::TemplateOnce;
use time::OffsetDateTime;
use tokio::fs;
use tokio::io::BufWriter;
use tokio::task::spawn_blocking;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::auth::Claims;
use crate::config::CONFIG;
use crate::errors::ServerError;
use crate::templates;

pub const ROUTE_PATH: &str = "/files/{*file_path}";
pub const ROUTE_PATH_ROOT: &str = "/files";
pub const ROUTE_PATH_ROOT_EMPTY: &str = "/files/";

pub async fn root_get(claims: Claims) -> Result<impl IntoResponse, ServerError> {
    get(claims, Path("".to_string())).await
}

pub async fn get(
    claims: Claims,
    Path(path): Path<String>,
) -> Result<impl IntoResponse, ServerError> {
    let full_path = CONFIG.store_path.join(&path);

    let metadata = fs::metadata(&full_path).await?;

    if metadata.is_dir() {
        let entries = spawn_blocking::<_, Result<_, IoError>>(move || {
            let mut entries = std::fs::read_dir(&full_path)?
                .map(|entry| {
                    let entry = entry?;
                    let entry_metadata = entry.metadata()?;

                    Ok(templates::FilesEntry {
                        name: entry.file_name().to_string_lossy().to_string(),
                        modified: OffsetDateTime::from(entry_metadata.modified()?),
                        is_dir: entry_metadata.is_dir(),
                        size: if entry_metadata.is_dir() {
                            0
                        } else {
                            entry_metadata.len()
                        },
                    })
                })
                .collect::<Result<Vec<_>, IoError>>()?;

            entries.sort_unstable();

            Ok(entries)
        })
        .await??;

        let upload_uri = if path.is_empty() {
            ROUTE_PATH_ROOT_EMPTY.to_string()
        } else if path.ends_with('/') {
            format!("{ROUTE_PATH_ROOT}/{path}")
        } else {
            format!("{ROUTE_PATH_ROOT}/{path}/")
        };

        let files_template = templates::Files {
            claims,
            path_prefix: ROUTE_PATH_ROOT,
            path: &path,
            entries,
            upload_uri,
        };

        Ok(Html(files_template.render_once()?).into_response())
    } else {
        let file = fs::File::open(&full_path).await?;
        let mime = mime_guess::from_path(&full_path)
            .first_or_octet_stream()
            .to_string();
        Ok((
            [
                (CONTENT_TYPE, mime),
                (CONTENT_LENGTH, metadata.len().to_string()),
            ],
            Body::from_stream(ReaderStream::new(file)),
        )
            .into_response())
    }
}

pub async fn root_post(
    claims: Claims,
    uri: OriginalUri,
    multipart: Multipart,
) -> Result<impl IntoResponse, ServerError> {
    post(claims, Path("".to_string()), uri, multipart).await
}

pub async fn post(
    _: Claims,
    Path(path): Path<String>,
    OriginalUri(uri): OriginalUri,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, ServerError> {
    let dir_path = CONFIG.store_path.join(&path);

    let field = multipart
        .next_field()
        .await?
        .ok_or(ServerError::InvalidUploadForm)?;

    if field.name() != Some("file") {
        return Err(ServerError::InvalidUploadForm);
    }
    let file_name = if let Some(file_name) = field.file_name() {
        file_name.to_string()
    } else {
        return Err(ServerError::InvalidUploadForm);
    };

    let file_path = dir_path.join(format!("{file_name}.form-upload"));

    let body_with_io_error = field.map_err(|err| IoError::new(IoErrorKind::Other, err));
    let body_reader = StreamReader::new(body_with_io_error);
    pin_mut!(body_reader);
    let mut file = BufWriter::new(fs::File::create(&file_path).await?);

    tokio::io::copy(&mut body_reader, &mut file).await?;

    let final_file_path = {
        let mut final_file_path = file_path.clone();
        final_file_path.set_file_name(file_name);
        final_file_path
    };

    fs::rename(file_path, final_file_path).await?;

    Ok(Html(
        templates::Redirect {
            title: "Upload Successful",
            url: uri.path(),
            success: true,
            message: "Now you'll be redirected to the file explorer.",
            ..Default::default()
        }
        .render_once()?,
    ))
}
