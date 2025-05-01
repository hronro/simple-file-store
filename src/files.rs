use std::io::{Error as IoError, ErrorKind as IoErrorKind};

use axum::body::Body;
use axum::extract::{Multipart, OriginalUri, Path};
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::{Html, IntoResponse};
use futures::{TryStreamExt, pin_mut};
use size::Size;
use time::{OffsetDateTime, macros::format_description};
use tokio::fs;
use tokio::io::BufWriter;
use tokio::task::spawn_blocking;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::auth::Claims;
use crate::config::CONFIG;
use crate::errors::ServerError;
use crate::html::redirect::{HtmlRedirectConfig, gen_html_redirect};

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
        let path_clone = path.clone();

        let files = spawn_blocking::<_, Result<_, IoError>>(move || {
            // Tupples for sorting
            // (is_file, file_name, html)
            let mut files = std::fs::read_dir(&full_path)?
                .map(|entry| {
                    let entry  = entry?;

                    let entry_metadata = entry.metadata()?;

                    let modified = OffsetDateTime::from(entry_metadata.modified()?).format(format_description!("[year]-[month]-[day] [hour]:[minute]:[second]")).unwrap();

                    let file_name = entry.file_name().to_string_lossy().to_string();

                    let uri = if path_clone.is_empty() {
                        format!("{ROUTE_PATH_ROOT}/{file_name}")
                    } else {
                        format!("{ROUTE_PATH_ROOT}/{path_clone}/{file_name}")
                    };

                    if entry_metadata.is_dir() {
                        Ok((false, file_name.clone(), format!(r###"<li><a class="folder-item" href="{uri}"><div class="folder-icon">📁</div><div class="file-details"><div class="folder-name">{file_name}</div><div class="file-meta"><span class="file-date">Modified: {modified}</span></div></div></a></li>"###)))
                    } else {
                        let ext = if let Some((_, extension)) = file_name.rsplit_once('.') { extension.to_uppercase() } else { "".to_string() };
                        let (file_icon, file_icon_class) = match ext.to_lowercase().as_str() {
                            "pdf" => ("📔", "pdf"),
                            "jpg" | "jpeg" | "png" | "apng" | "gif" | "svg" | "webp" | "avif" | "heif" | "bmp" => ("🖼", "image"),
                            "txt" | "doc" | "docx" | "pages" => ("📑", "doc"),
                            "js" | "css" | "c" | "cpp" | "rs" | "json" | "yaml" | "py" | "java" | "cs" | "rb" | "lua" => ("🖥️", "code"),
                            "zip" | "7z" | "gz" | "xz" | "tar" => ("📦", "archive"),
                            _ => ("📃", "other")
                        };
                        let size = Size::from_bytes(entry_metadata.len());
                        Ok((true, file_name.clone(), format!(r###"<li><a href="{uri}" target="_blank" class="file-item"><div class="file-icon {file_icon_class}">{file_icon}</div><div class="file-details"><div class="file-name">{file_name}</div><div class="file-meta"><span class="file-size">{size}</span><span class="file-date">{modified}</span></div></div></a><div class="file-actions"><a href="{uri}" download="{file_name}" class="download-btn">Download</a></div></li>"###)))
                    }
                })
                .collect::<Result<Vec<_>, IoError>>()?;

            files.sort_unstable_by(|x, y| {
                if x.0 != y.0 {
                    x.0.cmp(&y.0)
                } else {
                    x.1.cmp(&y.1)
                }
            });

            Ok(files.into_iter().map(|file| file.2).collect::<String>())
        })
        .await??;

        let user = claims.sub;
        let user_avatar = user.get(0..1).unwrap().to_uppercase();
        let breadcrumb = {
            let mut current_path = "/files".to_string();

            let mut dir_links = vec!["<a href=\"/files\">Home</a>".to_string()];

            for dir in path.clone().split('/').filter(|s| !s.is_empty()) {
                current_path.push('/');
                current_path.push_str(dir);
                dir_links.push(format!("<a href=\"{current_path}\">{dir}</a>"));
            }

            dir_links.join("<span class=\"breadcrumb-separator\">›</span>")
        };

        let content = if files.is_empty() {
            r###"<div class="empty-folder"><div class="empty-folder-icon">📂</div><div class="empty-folder-message">This folder is empty</div><div class="empty-folder-submessage">No files or folders to display</div></div>"###.to_string()
        } else {
            format!(r###"<ul class="file-list">{files}</ul>"###)
        };

        let upload_uri = if path.is_empty() {
            ROUTE_PATH_ROOT_EMPTY.to_string()
        } else if path.ends_with('/') {
            format!("{ROUTE_PATH_ROOT}/{path}")
        } else {
            format!("{ROUTE_PATH_ROOT}/{path}/")
        };

        Ok(Html(format!(
            r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>File Explorer</title>
<link rel="stylesheet" href="/_assets/reset.css">
<link rel="stylesheet" href="/_assets/files.css">
<script defer src="/_assets/upload.js"></script>
</head>
<body>
<div class="circles">
	<div class="circle circle-1"></div>
	<div class="circle circle-2"></div>
</div>

<header class="header">
	<div class="header-content">
		<h1>File Explorer</h1>
		<div class="user-info">
			<span>{user}</span>
			<div class="user-avatar">{user_avatar}</div>
		</div>
	</div>
</header>

<main class="main-container">
	<div class="explorer-card">
		<div class="breadcrumb">{breadcrumb}<button class="upload-btn" id="uploadBtn" command="show-modal" commandfor="uploadDialog">Upload</button></div>
                {content}
	</div>
</main>

<dialog id="uploadDialog" class="upload-dialog">
	<div class="dialog-header">
		<h3 class="dialog-title">Upload Files</h3>
		<form method="dialog">
			<button class="close-button">×</button>
		</form>
	</div>

	<div class="dialog-body">
		<form class="upload-form" id="uploadForm" method="POST" enctype="multipart/form-data" action="{upload_uri}">
			<div class="form-group" id="inputGroup">
				<label for="fileInput">Select File</label>
				<div class="file-input-wrapper">
					<div class="file-input-icon">📁</div>
					<div class="file-input-text">
						<noscript>Select a file to upload</noscript>
						<span class="js-only">Drag and drop or click to select</span>
					</div>
					<input type="file" id="fileInput" class="file-input" name="file" required>
				</div>
			</div>

			<div class="form-group option-group">
				<label class="upload-switch">
					<!-- Disabled by default, enabled by JS when available -->
					<input type="checkbox" name="resumableUpload" id="resumableUpload" disabled>
					<span class="switch-slider"></span>
				</label>
				<div class="option-label">
					<span class="option-title">Enable resumable upload</span>
					<span class="option-description">Continue uploads even if connection is interrupted</span>
					<span class="js-notice">Requires JavaScript to be enabled</span>
				</div>
			</div>

			<!-- Upload progress (shown via JS) -->
			<div class="form-group" >
				<div class="progress-container" id="uploadProgressContainer">
					<div class="progress-info">
						<div class="progress-status">Uploading...</div>
						<div class="progress-percentage" id="uploadProgressText">0%</div>
					</div>
					<div class="progress-bar-container">
						<div class="progress-bar" id="uploadProgressBar"></div>
					</div>
				</div>
			</div>

			<div class="dialog-footer">
				<button type="submit" class="btn btn-primary" id="uploadSubmitBtn">Upload</button>
			</div>
		</form>
	</div>
</dialog>
</body>
</html>"###
        ))
        .into_response())
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

    Ok(Html(gen_html_redirect(HtmlRedirectConfig {
        title: "Upload Successful",
        url: uri.path(),
        success: true,
        message: "Now you'll be redirected to the file explorer.",
        ..Default::default()
    })))
}
