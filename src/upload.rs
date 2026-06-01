use std::collections::HashMap;
use std::fs::{File as StdFile, OpenOptions as StdOpenOptions};
use std::io::BufReader;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use axum::body::Body;
use axum::extract::Path;
use axum::http::{StatusCode, header::HeaderMap};
use axum::response::{IntoResponse, Json};
use futures::StreamExt;
use rustix::fd::AsFd;
use rustix::fs::{FallocateFlags, FlockOperation, fallocate, flock};
use rustix::io::pwrite;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tokio::fs;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::spawn_blocking;

use crate::auth::Claims;
use crate::config::CONFIG;
use crate::errors::ServerError;
use crate::safe_path::safe_join;

pub const ROUTE_PATH: &str = "/upload/{*file_path}";
const UPLOAD_BYTE_BUDGET_UNIT: usize = 1024 * 1024;

static ACTIVE_UPLOAD_CHUNKS: LazyLock<Arc<Semaphore>> =
    LazyLock::new(|| Arc::new(Semaphore::new(CONFIG.max_active_upload_chunks)));
static ACTIVE_UPLOAD_BYTES: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    Arc::new(Semaphore::new(
        byte_budget_permits(CONFIG.max_active_upload_bytes) as usize,
    ))
});
/// Per-upload-path semaphores, held by `Weak` so they self-collect once every
/// in-flight `UploadPermits` for that path is dropped — no manual map cleanup,
/// no `Arc::strong_count` games. Stale entries (where the `Weak` can no longer
/// upgrade) are swept opportunistically on the next acquire for that path.
static ACTIVE_CHUNKS_BY_UPLOAD: LazyLock<Mutex<HashMap<PathBuf, Weak<Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Serialize_repr, Deserialize_repr, Debug, Clone, Copy, PartialEq, Eq)]
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
            // The first `unwrap()` is safe because `file_path` is guaranteed to have a file name,
            // and for the second `unwrap()`, since we are unlikely to support OSes that allow
            // non-UTF-8 file names, it's acceptable to panic if the file name is not valid UTF-8.
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

    /// Atomically read, mutate, and write the `.resumable-meta` file under an
    /// exclusive file lock. The updater may veto the write by returning an
    /// error; in that case the meta file is left untouched and the error is
    /// propagated to the caller. This is the only safe place to do
    /// check-and-set on chunk status.
    ///
    /// If the updater leaves the meta byte-identical to what was on disk, no
    /// write is performed — `set_len(0)` is only called when we actually have
    /// new bytes to write, so the no-op path is free and safe.
    pub async fn update_meta_file<F, U>(file_path: F, updater: U) -> Result<(), ServerError>
    where
        F: AsRef<std::path::Path>,
        U: FnOnce(&mut Self) -> Result<(), ServerError> + Send + 'static,
    {
        let meta_file_path = Self::path(file_path.as_ref());

        spawn_blocking(move || {
            let mut meta_file = StdOpenOptions::new()
                .read(true)
                .write(true)
                .open(&meta_file_path)
                .map_err(|err| ServerError::Custom {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    message: err.to_string(),
                })?;

            flock(meta_file.as_fd(), FlockOperation::LockExclusive)?;

            let mut meta = Self::read_from_file_sync(&meta_file)?;
            let before = serde_json::to_vec(&meta).unwrap();
            updater(&mut meta)?;
            let after = serde_json::to_vec(&meta).unwrap();

            if before != after {
                meta_file.set_len(0)?;
                meta_file.seek(SeekFrom::Start(0))?;
                meta_file.write_all(&after)?;
            }

            flock(meta_file.as_fd(), FlockOperation::Unlock)?;

            Ok(())
        })
        .await?
    }
}

/// Reset a chunk back to `NotStarted`, but **only** if it is still `Ongoing`.
/// The defensive check guards against the race where a retry has already
/// arrived and re-completed the chunk before this cleanup runs.
async fn reset_ongoing_chunk_to_not_started(file_path: PathBuf, chunk_index: usize) {
    let _ = ResumableUploadedFileMeta::update_meta_file(file_path, move |meta| {
        if let Some(status) = meta.chunks.get(&chunk_index)
            && status.is_ongoing()
        {
            meta.chunks.insert(chunk_index, ChunkStatus::NotStarted);
        }
        Ok(())
    })
    .await;
}

/// RAII safety net for the `Ongoing` chunk state. Created right after the
/// atomic transition to `Ongoing`; commit before returning from the happy or
/// explicit-error path. If the request future is dropped (cancellation,
/// panic-unwind in dev) before `commit` is called, `Drop` fires a tokio task
/// that resets the chunk to `NotStarted`.
///
/// Note: in release builds `panic = "abort"` skips `Drop` entirely, and
/// SIGKILL / power loss bypass it too — those cases are covered by
/// `reset_stale_ongoing_chunks` at startup.
struct OngoingChunkGuard {
    file_path: PathBuf,
    chunk_index: usize,
    runtime: tokio::runtime::Handle,
    committed: bool,
}

impl OngoingChunkGuard {
    fn new(file_path: PathBuf, chunk_index: usize) -> Self {
        Self {
            file_path,
            chunk_index,
            runtime: tokio::runtime::Handle::current(),
            committed: false,
        }
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for OngoingChunkGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let file_path = std::mem::take(&mut self.file_path);
        let chunk_index = self.chunk_index;
        self.runtime
            .spawn(reset_ongoing_chunk_to_not_started(file_path, chunk_index));
    }
}

/// Recursively walk `root`, resetting every `Ongoing` chunk in every
/// `.resumable-meta` file back to `NotStarted`. Run at process startup,
/// **before** the listener accepts traffic, so concurrent uploads cannot race
/// the cleanup. Tolerates a missing root (fresh install) and per-file errors
/// (corrupt meta, permission denied) — they are logged and skipped.
pub async fn reset_stale_ongoing_chunks(root: &StdPath) {
    let root = root.to_path_buf();
    let meta_paths = match spawn_blocking(move || find_meta_files(&root)).await {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("startup scan: failed to walk store path: {err}");
            return;
        }
    };

    for meta_path in meta_paths {
        let Some(file_name) = meta_path.file_stem() else {
            continue;
        };
        let original_path = meta_path.with_file_name(file_name);
        if let Err(err) = ResumableUploadedFileMeta::update_meta_file(&original_path, |meta| {
            for status in meta.chunks.values_mut() {
                if status.is_ongoing() {
                    *status = ChunkStatus::NotStarted;
                }
            }
            Ok(())
        })
        .await
        {
            eprintln!(
                "startup scan: skipping {}: {}",
                meta_path.display(),
                describe_server_error(&err),
            );
        }
    }
}

fn find_meta_files(root: &StdPath) -> Vec<PathBuf> {
    let mut metas = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("resumable-meta") {
                metas.push(path);
            }
        }
    }
    metas
}

fn describe_server_error(err: &ServerError) -> &'static str {
    match err {
        ServerError::IoError(_) => "io error",
        ServerError::UploadMetaIsBroken => "meta file is corrupt",
        ServerError::Custom { .. } => "internal error",
        _ => "error",
    }
}

/// Get the meta information of a resumable uploaded file.
pub async fn get(_: Claims, Path(path): Path<String>) -> Result<impl IntoResponse, ServerError> {
    let file_path = safe_join(&CONFIG.store_path, &path)?;

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
    let file_path = safe_join(&CONFIG.store_path, &path)?;

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
    fs::write(&upload_meta_file_path, upload_meta_file_content).await?;

    let upload_file_name = format!(
        "{}.resumable-upload",
        file_path.file_name().unwrap().to_str().unwrap()
    );
    let upload_file_path = file_path.with_file_name(upload_file_name);
    let upload_file = fs::File::create(&upload_file_path).await?;
    spawn_blocking(move || {
        fallocate(
            upload_file.as_fd(),
            FallocateFlags::empty(),
            0,
            request.size,
        )
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
    body: Body,
) -> Result<impl IntoResponse, ServerError> {
    let file_path = safe_join(&CONFIG.store_path, &path)?;

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

    // `chunk_size` and `chunks.len()` are fixed at meta creation, so validating
    // them outside the lock is safe. The chunk *status* check has to happen
    // atomically with the transition to `Ongoing`, so it lives in the closure
    // passed to `update_meta_file`.
    if content_length != meta.chunk_size
        && !(chunk_index == meta.chunks.len() - 1 && content_length < meta.chunk_size)
    {
        return Err(ServerError::InvalidContentLength);
    }
    if !meta.chunks.contains_key(&chunk_index) {
        return Err(ServerError::InvalidChunkIndex);
    }

    let _upload_permits = UploadPermits::acquire(&file_path, content_length)?;

    ResumableUploadedFileMeta::update_meta_file(&file_path, move |meta| {
        match meta.chunks.get(&chunk_index) {
            None => return Err(ServerError::InvalidChunkIndex),
            Some(s) if s.is_ongoing() => return Err(ServerError::ChunkIsOngoing),
            Some(s) if s.is_completed() => return Err(ServerError::ChunkIsCompleted),
            Some(_) => {}
        }
        meta.chunks.insert(chunk_index, ChunkStatus::Ongoing);
        Ok(())
    })
    .await?;

    // Safety net: if this future is cancelled (panic-unwind in dev, runtime
    // shutdown), Drop spawns a task to reset the chunk back to NotStarted.
    // MUST be constructed synchronously here — no `.await` between the
    // transition to Ongoing and this line, or there's a cancellation window.
    let guard = OngoingChunkGuard::new(file_path.clone(), chunk_index);

    let upload_file = fs::OpenOptions::new()
        .write(true)
        .open(&upload_file_path)
        .await?
        .into_std()
        .await;

    let write_result = stream_body_to_file(
        Arc::new(upload_file),
        (chunk_index * meta.chunk_size) as u64,
        content_length,
        body,
    )
    .await;

    if write_result.is_err() {
        ResumableUploadedFileMeta::update_meta_file(&file_path, move |meta| {
            meta.chunks.insert(chunk_index, ChunkStatus::NotStarted);
            Ok(())
        })
        .await?;
        guard.commit();
        return Err(ServerError::UploadChunkFailed);
    }

    ResumableUploadedFileMeta::update_meta_file(&file_path, move |meta| {
        meta.chunks.insert(chunk_index, ChunkStatus::Completed);
        Ok(())
    })
    .await?;
    guard.commit();

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

struct UploadPermits {
    // Fields are held only for their `Drop` side effect (releasing the permits
    // back to their respective semaphores when this struct goes out of scope).
    _global_permit: OwnedSemaphorePermit,
    _upload_permit: OwnedSemaphorePermit,
    _byte_budget_permit: OwnedSemaphorePermit,
}

impl UploadPermits {
    fn acquire(upload_path: &StdPath, content_length: usize) -> Result<Self, ServerError> {
        let global_permit = ACTIVE_UPLOAD_CHUNKS
            .clone()
            .try_acquire_owned()
            .map_err(|_| ServerError::TooManyUploadRequests)?;
        let byte_budget_permit = ACTIVE_UPLOAD_BYTES
            .clone()
            .try_acquire_many_owned(byte_budget_permits(content_length))
            .map_err(|_| ServerError::TooManyUploadRequests)?;

        let upload_semaphore = Self::get_or_create_upload_semaphore(upload_path);
        let upload_permit = upload_semaphore
            .try_acquire_owned()
            .map_err(|_| ServerError::TooManyUploadRequests)?;

        Ok(Self {
            _global_permit: global_permit,
            _upload_permit: upload_permit,
            _byte_budget_permit: byte_budget_permit,
        })
    }

    /// Returns the live `Arc<Semaphore>` for `upload_path`, creating a new one
    /// if the map has no entry yet or the existing `Weak` can no longer be
    /// upgraded. The pure map-manipulation logic lives in
    /// [`get_or_create_in_map`] so it can be unit-tested without touching the
    /// global statics.
    fn get_or_create_upload_semaphore(upload_path: &StdPath) -> Arc<Semaphore> {
        let mut map = ACTIVE_CHUNKS_BY_UPLOAD.lock().unwrap();
        get_or_create_in_map(&mut map, upload_path, CONFIG.max_active_chunks_per_upload)
    }
}

/// Get the live `Arc<Semaphore>` for `upload_path` from `map`, creating a new
/// one (sized at `max_permits`) when the entry is absent or its `Weak` can no
/// longer be upgraded. Dead entries for *other* paths are swept on the cold
/// path only (when we already have to allocate), bounding map growth without
/// an extra O(n) sweep on every acquire.
fn get_or_create_in_map(
    map: &mut HashMap<PathBuf, Weak<Semaphore>>,
    upload_path: &StdPath,
    max_permits: usize,
) -> Arc<Semaphore> {
    if let Some(arc) = map.get(upload_path).and_then(Weak::upgrade) {
        return arc;
    }

    map.retain(|_, weak| weak.strong_count() > 0);
    let arc = Arc::new(Semaphore::new(max_permits));
    map.insert(upload_path.to_path_buf(), Arc::downgrade(&arc));
    arc
}

fn byte_budget_permits(byte_count: usize) -> u32 {
    byte_count
        .div_ceil(UPLOAD_BYTE_BUDGET_UNIT)
        .max(1)
        .min(u32::MAX as usize) as u32
}

async fn stream_body_to_file(
    file: Arc<StdFile>,
    offset: u64,
    content_length: usize,
    body: Body,
) -> Result<(), ServerError> {
    let mut body = body.into_data_stream();
    let mut total_written = 0usize;

    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|err| ServerError::Custom {
            status: StatusCode::BAD_REQUEST,
            message: err.to_string(),
        })?;

        let chunk_len = chunk.len();
        if total_written + chunk_len > content_length {
            return Err(ServerError::InvalidContentLength);
        }

        let file = file.clone();
        let chunk_offset = offset + total_written as u64;
        spawn_blocking(move || file_seek_write_all(&file, chunk_offset, &chunk)).await??;
        total_written += chunk_len;
    }

    if total_written != content_length {
        return Err(ServerError::InvalidContentLength);
    }

    Ok(())
}

#[cfg(not(windows))]
#[inline]
fn file_seek_write_all(file: &StdFile, offset: u64, data: &[u8]) -> Result<(), ServerError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    // ----- helpers -----

    fn write_meta_raw(file_path: &StdPath, meta: &ResumableUploadedFileMeta) {
        let meta_path = ResumableUploadedFileMeta::path(file_path);
        let bytes = serde_json::to_vec(meta).unwrap();
        std::fs::write(meta_path, bytes).unwrap();
    }

    fn read_meta_raw(file_path: &StdPath) -> ResumableUploadedFileMeta {
        let meta_path = ResumableUploadedFileMeta::path(file_path);
        let file = StdFile::open(meta_path).unwrap();
        serde_json::from_reader(BufReader::new(file)).unwrap()
    }

    fn fixture(chunks: &[ChunkStatus]) -> ResumableUploadedFileMeta {
        let chunk_size = 8;
        let file_size = (chunks.len() * chunk_size) as u64;
        let mut meta = ResumableUploadedFileMeta::new(chunk_size, file_size);
        for (i, status) in chunks.iter().enumerate() {
            meta.chunks.insert(i, *status);
        }
        meta
    }

    async fn await_predicate<F: FnMut() -> bool>(mut predicate: F) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !predicate() {
            if Instant::now() > deadline {
                panic!("predicate did not become true within 2s");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // ----- ResumableUploadedFileMeta::new chunk-count math -----

    #[test]
    fn new_zero_size_yields_zero_chunks() {
        let meta = ResumableUploadedFileMeta::new(8 * 1024 * 1024, 0);
        assert_eq!(meta.chunks.len(), 0);
        assert_eq!(meta.file_size, 0);
    }

    #[test]
    fn new_single_byte_yields_one_chunk() {
        let meta = ResumableUploadedFileMeta::new(8 * 1024 * 1024, 1);
        assert_eq!(meta.chunks.len(), 1);
        assert_eq!(meta.chunks[&0], ChunkStatus::NotStarted);
    }

    #[test]
    fn new_exact_chunk_size_yields_one_chunk() {
        let meta = ResumableUploadedFileMeta::new(8 * 1024 * 1024, 8 * 1024 * 1024);
        assert_eq!(meta.chunks.len(), 1);
    }

    #[test]
    fn new_chunk_size_plus_one_yields_two_chunks() {
        let meta = ResumableUploadedFileMeta::new(8 * 1024 * 1024, 8 * 1024 * 1024 + 1);
        assert_eq!(meta.chunks.len(), 2);
    }

    #[test]
    fn new_exact_two_chunks() {
        let meta = ResumableUploadedFileMeta::new(8 * 1024 * 1024, 16 * 1024 * 1024);
        assert_eq!(meta.chunks.len(), 2);
    }

    #[test]
    fn new_two_chunks_plus_one_yields_three() {
        let meta = ResumableUploadedFileMeta::new(8 * 1024 * 1024, 16 * 1024 * 1024 + 1);
        assert_eq!(meta.chunks.len(), 3);
    }

    // ----- path round-trip via file_stem -----

    #[test]
    fn meta_path_round_trip_with_extension() {
        let original = StdPath::new("/store/foo.txt");
        let meta = ResumableUploadedFileMeta::path(original);
        assert_eq!(meta, StdPath::new("/store/foo.txt.resumable-meta"));
        let derived = meta.with_file_name(meta.file_stem().unwrap());
        assert_eq!(derived, original);
    }

    #[test]
    fn meta_path_round_trip_no_extension() {
        let original = StdPath::new("/store/foo");
        let meta = ResumableUploadedFileMeta::path(original);
        assert_eq!(meta, StdPath::new("/store/foo.resumable-meta"));
        let derived = meta.with_file_name(meta.file_stem().unwrap());
        assert_eq!(derived, original);
    }

    #[test]
    fn meta_path_round_trip_multi_dot() {
        let original = StdPath::new("/store/foo.tar.gz");
        let meta = ResumableUploadedFileMeta::path(original);
        assert_eq!(meta, StdPath::new("/store/foo.tar.gz.resumable-meta"));
        let derived = meta.with_file_name(meta.file_stem().unwrap());
        assert_eq!(derived, original);
    }

    // ----- update_meta_file -----

    #[tokio::test]
    async fn update_meta_file_applies_change() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::NotStarted]));

        ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            meta.chunks.insert(0, ChunkStatus::Completed);
            Ok(())
        })
        .await
        .unwrap();

        assert_eq!(read_meta_raw(&file_path).chunks[&0], ChunkStatus::Completed);
    }

    #[tokio::test]
    async fn update_meta_file_does_not_write_when_updater_errs() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::NotStarted]));
        let meta_path = ResumableUploadedFileMeta::path(&file_path);
        let before = std::fs::read(&meta_path).unwrap();

        let result = ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            meta.chunks.insert(0, ChunkStatus::Ongoing);
            Err(ServerError::ChunkIsOngoing)
        })
        .await;

        assert!(matches!(result, Err(ServerError::ChunkIsOngoing)));
        let after = std::fs::read(&meta_path).unwrap();
        assert_eq!(before, after, "meta file must be byte-identical after err");
    }

    #[tokio::test]
    async fn update_meta_file_skips_write_on_noop() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Completed]));
        let meta_path = ResumableUploadedFileMeta::path(&file_path);
        let before = std::fs::read(&meta_path).unwrap();
        let before_mtime = std::fs::metadata(&meta_path).unwrap().modified().unwrap();

        // Sleep so any spurious write would produce a measurably different mtime.
        tokio::time::sleep(Duration::from_millis(20)).await;

        ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |_meta| Ok(()))
            .await
            .unwrap();

        let after = std::fs::read(&meta_path).unwrap();
        let after_mtime = std::fs::metadata(&meta_path).unwrap().modified().unwrap();
        assert_eq!(before, after);
        assert_eq!(before_mtime, after_mtime, "no-op must not touch the file");
    }

    #[tokio::test]
    async fn update_meta_file_truncates_when_shrinking() {
        // Pad the on-disk file with trailing whitespace (which serde_json
        // tolerates) so a subsequent write that doesn't truncate would leave
        // those trailing bytes behind. With the truncate fix the file must
        // shrink back to exactly the serialized length.
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        let meta_path = ResumableUploadedFileMeta::path(&file_path);

        write_meta_raw(&file_path, &fixture(&[ChunkStatus::NotStarted]));
        let mut padded = std::fs::read(&meta_path).unwrap();
        padded.extend(std::iter::repeat_n(b' ', 64));
        std::fs::write(&meta_path, &padded).unwrap();

        ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            meta.chunks.insert(0, ChunkStatus::Completed);
            Ok(())
        })
        .await
        .unwrap();

        let expected = serde_json::to_vec(&read_meta_raw(&file_path)).unwrap();
        assert_eq!(
            std::fs::metadata(&meta_path).unwrap().len(),
            expected.len() as u64,
            "file must be exactly the new serialized length (no trailing padding)"
        );
        assert_eq!(std::fs::read(&meta_path).unwrap(), expected);
    }

    // ----- State transitions / chunk merge predicate -----

    #[tokio::test]
    async fn cannot_transition_to_ongoing_twice() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::NotStarted]));

        ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            meta.chunks.insert(0, ChunkStatus::Ongoing);
            Ok(())
        })
        .await
        .unwrap();

        // Simulate the put() handler's check-and-set closure.
        let result = ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            match meta.chunks.get(&0) {
                Some(s) if s.is_ongoing() => return Err(ServerError::ChunkIsOngoing),
                _ => {}
            }
            meta.chunks.insert(0, ChunkStatus::Ongoing);
            Ok(())
        })
        .await;

        assert!(matches!(result, Err(ServerError::ChunkIsOngoing)));
    }

    #[tokio::test]
    async fn cannot_transition_completed_to_ongoing() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Completed]));

        let result = ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            match meta.chunks.get(&0) {
                Some(s) if s.is_completed() => return Err(ServerError::ChunkIsCompleted),
                _ => {}
            }
            meta.chunks.insert(0, ChunkStatus::Ongoing);
            Ok(())
        })
        .await;

        assert!(matches!(result, Err(ServerError::ChunkIsCompleted)));
    }

    #[tokio::test]
    async fn all_completed_predicate_flips_on_last_chunk() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::NotStarted; 3]));

        for i in 0..2 {
            ResumableUploadedFileMeta::update_meta_file(file_path.clone(), move |meta| {
                meta.chunks.insert(i, ChunkStatus::Completed);
                Ok(())
            })
            .await
            .unwrap();
            assert!(
                !read_meta_raw(&file_path)
                    .chunks
                    .values()
                    .all(|s| s.is_completed()),
                "predicate must stay false until the final chunk"
            );
        }

        ResumableUploadedFileMeta::update_meta_file(file_path.clone(), |meta| {
            meta.chunks.insert(2, ChunkStatus::Completed);
            Ok(())
        })
        .await
        .unwrap();
        assert!(
            read_meta_raw(&file_path)
                .chunks
                .values()
                .all(|s| s.is_completed())
        );
    }

    // ----- reset_ongoing_chunk_to_not_started (free fn) -----

    #[tokio::test]
    async fn reset_ongoing_to_not_started_resets_ongoing() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Ongoing]));

        reset_ongoing_chunk_to_not_started(file_path.clone(), 0).await;

        assert_eq!(
            read_meta_raw(&file_path).chunks[&0],
            ChunkStatus::NotStarted
        );
    }

    #[tokio::test]
    async fn reset_ongoing_to_not_started_leaves_completed_alone() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Completed]));

        reset_ongoing_chunk_to_not_started(file_path.clone(), 0).await;

        assert_eq!(read_meta_raw(&file_path).chunks[&0], ChunkStatus::Completed);
    }

    #[tokio::test]
    async fn reset_ongoing_to_not_started_leaves_not_started_alone() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::NotStarted]));

        reset_ongoing_chunk_to_not_started(file_path.clone(), 0).await;

        assert_eq!(
            read_meta_raw(&file_path).chunks[&0],
            ChunkStatus::NotStarted
        );
    }

    #[tokio::test]
    async fn reset_ongoing_to_not_started_ignores_missing_meta() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("does-not-exist.bin");
        // No panic, no observable side effect.
        reset_ongoing_chunk_to_not_started(file_path, 0).await;
    }

    // ----- OngoingChunkGuard Drop semantics -----

    #[tokio::test]
    async fn guard_drop_without_commit_resets_chunk() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Ongoing]));

        {
            let _guard = OngoingChunkGuard::new(file_path.clone(), 0);
            // _guard drops here without commit
        }

        let fp = file_path.clone();
        await_predicate(|| read_meta_raw(&fp).chunks[&0] == ChunkStatus::NotStarted).await;
    }

    #[tokio::test]
    async fn guard_commit_prevents_reset() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Ongoing]));

        let guard = OngoingChunkGuard::new(file_path.clone(), 0);
        guard.commit();

        // Give any (incorrectly spawned) cleanup task plenty of time to run.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(read_meta_raw(&file_path).chunks[&0], ChunkStatus::Ongoing);
    }

    #[tokio::test]
    async fn guard_drop_does_not_clobber_completed() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("a.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Ongoing]));
        let guard = OngoingChunkGuard::new(file_path.clone(), 0);

        // Race scenario: a retry completed the chunk before our cleanup spawned.
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Completed]));
        drop(guard);

        // Wait long enough for any reset task to have run.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(read_meta_raw(&file_path).chunks[&0], ChunkStatus::Completed);
    }

    // ----- reset_stale_ongoing_chunks (startup scan) -----

    #[tokio::test]
    async fn startup_scan_resets_ongoing_in_nested_dirs() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let file_path = nested.join("file.bin");
        write_meta_raw(&file_path, &fixture(&[ChunkStatus::Ongoing]));

        reset_stale_ongoing_chunks(dir.path()).await;

        assert_eq!(
            read_meta_raw(&file_path).chunks[&0],
            ChunkStatus::NotStarted
        );
    }

    #[tokio::test]
    async fn startup_scan_only_touches_ongoing_chunks() {
        let dir = TempDir::new().unwrap();
        let file_path = dir.path().join("mix.bin");
        write_meta_raw(
            &file_path,
            &fixture(&[
                ChunkStatus::NotStarted,
                ChunkStatus::Ongoing,
                ChunkStatus::Completed,
            ]),
        );

        reset_stale_ongoing_chunks(dir.path()).await;

        let meta = read_meta_raw(&file_path);
        assert_eq!(meta.chunks[&0], ChunkStatus::NotStarted);
        assert_eq!(meta.chunks[&1], ChunkStatus::NotStarted);
        assert_eq!(meta.chunks[&2], ChunkStatus::Completed);
    }

    #[tokio::test]
    async fn startup_scan_ignores_non_meta_files() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("payload.bin.resumable-upload"), b"data").unwrap();
        std::fs::write(dir.path().join("unrelated.txt"), b"x").unwrap();

        // Should not panic, should not touch unrelated files.
        reset_stale_ongoing_chunks(dir.path()).await;

        assert_eq!(
            std::fs::read(dir.path().join("payload.bin.resumable-upload")).unwrap(),
            b"data"
        );
        assert_eq!(
            std::fs::read(dir.path().join("unrelated.txt")).unwrap(),
            b"x"
        );
    }

    #[tokio::test]
    async fn startup_scan_tolerates_missing_root() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("does-not-exist");
        // Must not panic.
        reset_stale_ongoing_chunks(&missing).await;
    }

    #[tokio::test]
    async fn startup_scan_skips_corrupt_meta_and_processes_valid() {
        let dir = TempDir::new().unwrap();
        let corrupt_path = dir.path().join("corrupt.bin");
        std::fs::write(
            ResumableUploadedFileMeta::path(&corrupt_path),
            b"not json at all",
        )
        .unwrap();

        let valid_path = dir.path().join("valid.bin");
        write_meta_raw(&valid_path, &fixture(&[ChunkStatus::Ongoing]));

        reset_stale_ongoing_chunks(dir.path()).await;

        assert_eq!(
            read_meta_raw(&valid_path).chunks[&0],
            ChunkStatus::NotStarted
        );
    }

    #[tokio::test]
    async fn startup_scan_empty_root_noop() {
        let dir = TempDir::new().unwrap();
        // Empty dir, no metas, no panic.
        reset_stale_ongoing_chunks(dir.path()).await;
    }

    // ----- get_or_create_in_map (Weak per-upload semaphore map) -----

    #[test]
    fn weak_map_creates_entry_when_empty() {
        let mut map = HashMap::new();
        let arc = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(StdPath::new("/store/a.bin")));
        assert_eq!(arc.available_permits(), 4);
    }

    #[test]
    fn weak_map_returns_same_arc_for_live_entry() {
        let mut map = HashMap::new();
        let arc1 = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        let arc2 = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        assert!(Arc::ptr_eq(&arc1, &arc2));
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn weak_map_replaces_dead_entry_for_same_path() {
        let mut map = HashMap::new();
        let arc1 = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        drop(arc1);
        // The Weak in the map is now dead.
        let weak_before = map.get(StdPath::new("/store/a.bin")).cloned().unwrap();
        assert_eq!(weak_before.strong_count(), 0);

        let arc2 = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        assert_eq!(arc2.available_permits(), 4);
        // The entry's Weak must point to the *new* allocation, not the dead one.
        let new_weak = map.get(StdPath::new("/store/a.bin")).unwrap();
        assert!(new_weak.upgrade().is_some());
        assert!(Arc::ptr_eq(&new_weak.upgrade().unwrap(), &arc2));
    }

    #[test]
    fn weak_map_distinct_paths_get_distinct_arcs() {
        let mut map = HashMap::new();
        let arc_a = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        let arc_b = get_or_create_in_map(&mut map, StdPath::new("/store/b.bin"), 4);
        assert!(!Arc::ptr_eq(&arc_a, &arc_b));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn weak_map_cold_path_sweeps_dead_entries_for_other_paths() {
        let mut map = HashMap::new();
        // Seed two dead Weaks for unrelated paths.
        let stale_a = Arc::new(Semaphore::new(1));
        let stale_b = Arc::new(Semaphore::new(1));
        map.insert(PathBuf::from("/store/stale_a"), Arc::downgrade(&stale_a));
        map.insert(PathBuf::from("/store/stale_b"), Arc::downgrade(&stale_b));
        drop(stale_a);
        drop(stale_b);
        assert_eq!(map.len(), 2);

        // Cold-path acquire for a new path triggers the sweep.
        let _arc = get_or_create_in_map(&mut map, StdPath::new("/store/new.bin"), 4);
        assert_eq!(map.len(), 1, "dead entries for other paths must be swept");
        assert!(map.contains_key(StdPath::new("/store/new.bin")));
    }

    #[test]
    fn weak_map_hot_path_does_not_sweep() {
        let mut map = HashMap::new();
        // Live entry for path A.
        let arc_a = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        // Dead Weak for path B, inserted by hand.
        let stale_b = Arc::new(Semaphore::new(1));
        map.insert(PathBuf::from("/store/b.bin"), Arc::downgrade(&stale_b));
        drop(stale_b);
        assert_eq!(map.len(), 2);

        // Hot-path acquire for A — should upgrade existing Weak and *not* sweep.
        let arc_a_again = get_or_create_in_map(&mut map, StdPath::new("/store/a.bin"), 4);
        assert!(Arc::ptr_eq(&arc_a, &arc_a_again));
        assert_eq!(
            map.len(),
            2,
            "hot path must not pay the O(n) sweep cost; stale B entry stays until cold path"
        );
    }
}
