use std::collections::HashMap;
use std::fs::{File as StdFile, OpenOptions as StdOpenOptions};
use std::io::BufReader;
use std::io::Write;
use std::path::PathBuf;

use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{StatusCode, header::HeaderMap};
use axum::response::{IntoResponse, Json};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tokio::fs;
use tokio::task::spawn_blocking;

#[cfg(not(windows))]
use rustix::fd::AsFd;
#[cfg(not(windows))]
use rustix::fs::{FallocateFlags, fallocate};

use crate::auth::Claims;
use crate::config::CONFIG;
use crate::errors::ServerError;

pub const ROUTE_PATH: &str = "/upload/{*file_path}";

#[derive(Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum ChunkStatus {
    NotStarted = 0,
    Ongoing = 1,
    Completed = 2,
}

impl ChunkStatus {
    #[allow(dead_code)]
    pub fn is_not_started(&self) -> bool {
        matches!(self, ChunkStatus::NotStarted)
    }

    #[allow(dead_code)]
    pub fn is_ongoing(&self) -> bool {
        matches!(self, ChunkStatus::Ongoing)
    }

    #[allow(dead_code)]
    pub fn is_completed(&self) -> bool {
        matches!(self, ChunkStatus::Completed)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumableUploadedFileMeta {
    chunk_size: usize,
    file_size: u64,
    chunks: HashMap<usize, ChunkStatus>,
}

impl ResumableUploadedFileMeta {
    /// Create a new instance of `ResumableUploadedFileMeta`.
    pub fn new(chunk_size: usize, file_size: u64) -> Self {
        let chunks_count = (file_size as f64 / chunk_size as f64).ceil() as usize;
        Self {
            chunk_size,
            file_size,
            chunks: (0..chunks_count)
                .map(|i| (i, ChunkStatus::NotStarted))
                .collect(),
        }
    }

    /// Return the meta file path for the given file path.
    pub fn path<T: AsRef<std::path::Path>>(file_path: T) -> PathBuf {
        let file_name = format!(
            "{}.resumable-meta",
            file_path.as_ref().file_name().unwrap().to_str().unwrap()
        );
        file_path.as_ref().with_file_name(file_name)
    }

    /// Check if the meta file exists for the given file path.
    pub fn exists<T: AsRef<std::path::Path>>(file_path: T) -> Result<bool, std::io::Error> {
        Self::path(file_path).try_exists()
    }

    /// Read the meta information from the given file path.
    pub async fn read_from_file<T: AsRef<std::path::Path>>(
        file_path: T,
    ) -> Result<Option<Self>, ServerError> {
        match fs::File::open(Self::path(file_path.as_ref())).await {
            Ok(tokio_file) => {
                let std_file = tokio_file.into_std().await;
                let meta = spawn_blocking(move || Self::read_from_file_sync(&std_file)).await??;
                Ok(Some(meta))
            }

            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),

            Err(err) => Err(err.into()),
        }
    }

    fn read_from_file_sync(meta_file: &StdFile) -> Result<Self, ServerError> {
        let reader = BufReader::new(meta_file);
        let meta: ResumableUploadedFileMeta =
            serde_json::from_reader(reader).map_err(|_| ServerError::UploadMetaIsBroken)?;

        Ok::<_, ServerError>(meta)
    }

    /// Update the `.resumable-meta` file for the given file path.
    /// During the update process, the file locked so no other process can read or write to it.
    pub async fn update_meta_file<F, U>(file_path: F, updater: U) -> Result<(), ServerError>
    where
        F: AsRef<std::path::Path>,
        U: FnOnce(&mut Self) + Send + 'static,
    {
        let meta_file_path = Self::path(file_path.as_ref());

        spawn_blocking(move || {
            let mut meta_file = StdOpenOptions::new()
                .write(true)
                .append(false)
                .open(&meta_file_path)
                .map_err(|err| ServerError::Custom {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: err.to_string(),
                })?;

            meta_file.lock_exclusive()?;

            let meta_file_for_read = StdFile::open(&meta_file_path)?;
            let mut meta = Self::read_from_file_sync(&meta_file_for_read)?;
            updater(&mut meta);
            let new_meta_file_content = serde_json::to_vec(&meta).unwrap();
            meta_file.write_all(&new_meta_file_content)?;

            FileExt::unlock(&meta_file)?;

            Ok(())
        })
        .await?
    }
}

/// Get the meta information of a resumable uploaded file.
pub async fn get(_: Claims, Path(path): Path<String>) -> Result<impl IntoResponse, ServerError> {
    let file_path = CONFIG.store_path.join(&path);

    if let Some(meta) = ResumableUploadedFileMeta::read_from_file(&file_path).await? {
        Ok(Json(meta).into_response())
    } else {
        Ok(StatusCode::NOT_FOUND.into_response())
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateResumableUploadFileRequest {
    pub size: u64,
}

/// Create a resumable upload file.
pub async fn post(
    _: Claims,
    Path(path): Path<String>,
    request: Json<CreateResumableUploadFileRequest>,
) -> Result<impl IntoResponse, ServerError> {
    let file_path = CONFIG.store_path.join(&path);

    if ResumableUploadedFileMeta::exists(&file_path)? {
        return Err(ServerError::FileAlreadyExists);
    }

    let upload_meta = ResumableUploadedFileMeta::new(CONFIG.chunk_size, request.size);
    let upload_meta_file_path = ResumableUploadedFileMeta::path(&file_path);
    let upload_meta_file_content =
        serde_json::to_string(&upload_meta).map_err(|err| ServerError::Custom {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        })?;
    fs::File::create(&upload_meta_file_path).await?;
    fs::write(&upload_meta_file_path, upload_meta_file_content).await?;

    let upload_file_name = format!(
        "{}.resumable-upload",
        file_path.file_name().unwrap().to_str().unwrap()
    );
    let upload_file_path = file_path.with_file_name(upload_file_name);
    let upload_file = fs::File::create(&upload_file_path).await?;
    
    spawn_blocking(move || {
        #[cfg(not(windows))]
        {
            fallocate(
                upload_file.as_fd(),
                FallocateFlags::empty(),
                0,
                request.size,
            )
        }
        #[cfg(windows)]
        {
            upload_file.set_len(request.size)
        }
    })
    .await??;

    Ok((StatusCode::CREATED, Json(upload_meta)))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumableUploadFileResponse {
    pub success: bool,
    pub all_chunks_completed: bool,
}
impl ResumableUploadFileResponse {
    pub fn new(success: bool, all_chunks_completed: bool) -> Self {
        Self {
            success,
            all_chunks_completed,
        }
    }
}

/// Create a resumable upload file.
pub async fn put(
    _: Claims,
    Path(path): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<impl IntoResponse, ServerError> {
    let file_path = CONFIG.store_path.join(&path);

    let upload_file_name = format!(
        "{}.resumable-upload",
        file_path.file_name().unwrap().to_str().unwrap()
    );
    let upload_file_path = file_path.with_file_name(upload_file_name);

    let chunk_index: usize = headers
        .get("resumable-upload-chunk-index")
        .ok_or(ServerError::MissingChunkIndex)?
        .to_str()
        .map_err(|_| ServerError::InvalidChunkIndex)?
        .parse()
        .map_err(|_| ServerError::InvalidChunkIndex)?;

    let content_length: usize = headers
        .get("content-length")
        .ok_or(ServerError::MissingContentLength)?
        .to_str()
        .map_err(|_| ServerError::InvalidContentLength)?
        .parse()
        .map_err(|_| ServerError::InvalidContentLength)?;

    let meta = ResumableUploadedFileMeta::read_from_file(&file_path)
        .await?
        .ok_or(ServerError::FileIsNotCreated)?;

    if content_length != meta.chunk_size {
        // Only the last chunk can be smaller than the chunk size.
        if !(chunk_index == meta.chunks.len() - 1 && content_length < meta.chunk_size) {
            return Err(ServerError::InvalidContentLength);
        }
    }

    let chunk_status = meta.chunks.get(&chunk_index);
    if chunk_status.is_none() {
        return Err(ServerError::InvalidChunkIndex);
    }
    let chunk_status = chunk_status.unwrap();

    if chunk_status.is_ongoing() {
        return Err(ServerError::ChunkIsOngoing);
    }

    if chunk_status.is_completed() {
        return Err(ServerError::ChunkIsCompleted);
    }

    ResumableUploadedFileMeta::update_meta_file(&file_path, move |meta| {
        meta.chunks.insert(chunk_index, ChunkStatus::Ongoing);
    })
    .await?;

    let upload_file = fs::OpenOptions::new()
        .write(true)
        .open(&upload_file_path)
        .await?
        .into_std()
        .await;

    let write_result = spawn_blocking(move || {
        file_seek_write_all(&upload_file, (chunk_index * meta.chunk_size) as u64, &body)
    })
    .await;

    if !matches!(write_result, Ok(Ok(_))) {
        ResumableUploadedFileMeta::update_meta_file(&file_path, move |meta| {
            meta.chunks.insert(chunk_index, ChunkStatus::NotStarted);
        })
        .await?;
        return Err(ServerError::UploadChunkFailed);
    }

    ResumableUploadedFileMeta::update_meta_file(&file_path, move |meta| {
        meta.chunks.insert(chunk_index, ChunkStatus::Completed);
    })
    .await?;

    // Check if all chunks are completed
    let meta = ResumableUploadedFileMeta::read_from_file(&file_path)
        .await?
        .unwrap();
    if meta.chunks.values().all(|status| status.is_completed()) {
        fs::rename(upload_file_path, &file_path).await?;
        fs::remove_file(ResumableUploadedFileMeta::path(&file_path)).await?;

        Ok(Json(ResumableUploadFileResponse::new(true, true)))
    } else {
        Ok(Json(ResumableUploadFileResponse::new(true, false)))
    }
}

#[cfg(not(windows))]
#[inline]
fn file_seek_write_all(file: &StdFile, offset: u64, data: &[u8]) -> Result<(), ServerError> {
    use rustix::fd::AsFd;
    use rustix::io::pwrite;
    
    let mut total_written = 0;

    while total_written < data.len() {
        let written = pwrite(
            file.as_fd(),
            &data[total_written..],
            offset + total_written as u64,
        )?;
        total_written += written;
    }

    Ok(())
}

#[cfg(windows)]
#[inline]
fn file_seek_write_all(file: &StdFile, offset: u64, data: &[u8]) -> Result<(), ServerError> {
    use std::os::windows::fs::FileExt;
    let mut total_written = 0;

    while total_written < data.len() {
        let written = file.write_at(&data[total_written..], offset + total_written as u64)?;
        total_written += written;
    }

    Ok(())
}
