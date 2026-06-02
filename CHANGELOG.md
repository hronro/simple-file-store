# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0]

### Added

- Server-side concurrency limits for resumable uploads, configurable with `SFS_MAX_ACTIVE_UPLOAD_CHUNKS`, `SFS_MAX_ACTIVE_CHUNKS_PER_UPLOAD`, and `SFS_MAX_ACTIVE_UPLOAD_BYTES`.
- HTTP `Range` request support in `GET /files/{path}`, enabling parallel and resumable downloads with clients like `aria2c -x` or `curl -r`.

### Changed

- Replaced the `GET /hello-world` debug route with `GET /ping`, which returns `{"pong":"<username>"}` for authenticated requests and `{"pong":null}` for unauthenticated ones, letting native and third-party clients verify their auth token.
- Stream resumable upload chunks directly to disk instead of buffering the full chunk in memory, reducing memory pressure during concurrent uploads.
- Return `429 Too Many Requests` with `Retry-After` when upload concurrency limits are reached, and make the web client retry affected chunks instead of failing the upload immediately.
- Upgrade Rust to v1.96.
- Upgrade dependencies to their latest versions.

### Fixed

- Race condition in `PUT /upload/{file}` where two concurrent requests for the same chunk could both pass the "chunk is available" check and write to the same byte range, corrupting the upload. The check is now atomic with the transition to the `Ongoing` status under the meta file lock.
- Orphaned `Ongoing` chunks blocking retries after a client disconnect, runtime cancellation, or process kill. A new RAII guard resets the chunk back to `NotStarted` if the request future is dropped before completion, and a startup scan walks the store path on boot to recover any chunks left `Ongoing` by a hard kill of the previous run.
- `update_meta_file` no longer leaves trailing bytes from the previous write when the new serialized meta is shorter than what was on disk. The file is now truncated to exactly the new content's length, and writes are skipped entirely when the updater leaves the meta unchanged.

### Changed

- Per-upload semaphores in the chunk concurrency map are now held by `Weak` references instead of `Arc`, removing the fragile `Arc::strong_count == 2` cleanup heuristic in `UploadPermits::Drop`. Stale entries are swept opportunistically on the next acquire for the same path. This also fixes a small map leak when `UploadPermits::acquire` failed after creating the entry but before successfully acquiring all permits.

### Security

- Reject `..`, absolute paths, and other non-normal path components in user-supplied file and directory paths, preventing directory traversal that could read or write files outside the configured store path. Affected endpoints: `/files/*`, `/upload/*`, and the multipart upload's `filename` field.

## [0.3.1]

### Changed

- Use the official `sailfish` crate instead of the forked version, since the official `sailfish` crate now supports removing newlines in the rendered HTML files.
- Upgrade Rust to v1.92.
- Upgrade to dependencies to their latest versions.
- Change the crypto backend to `aws_lc_rs`.

## [0.3.0]

### Added

- Website favicon.

### Changed

- Now the front-end code files (CSS and JavaScript) are minified, which leads to a smaller binary size and faster client side loading times.
- Newlines are removed in the rendered HTML files. We use `sailfish` as the template engine, and we had already enabled the `rm_whitespace` option in `sailfish` before, but for some reason the `rm_whitespace` option does not remove newlines in the rendered HTML files. Now we switched to a forked version of `sailfish` that supports removing newlines in the rendered HTML files.
- The number of upload worker has increased to 6 from 4, since all major browsers support a minimum of 6 parallel HTTP/1.1 connections.
- Upgrade Rust to v1.88.

## [0.2.0]

### Added

- `Content-Type` HTTP header for user uploaded files, guessing the MIME type based on the file extension.
- HTTP cache for built-in front-end assets.
- time counter for file upload.
- TLS support.

### Changed

- Split the file link and the download link. Previously whether you click the file itself or click the download button, it would always download the file. Now, clicking the file link will open the file in a new tab, while clicking the download button will download the file.
- Upgrade Rust to v1.87.
- Trim help messages in CLI commands to remove trailing newlines.

### Fixed

- Use correct HTTP status code (401) when authentication fails.

## [0.1.0]

### Added

- Command line interface (CLI) for managing the server.
- JWT-based authentication.
- Basic folder preview.
- File downloading.
- Normal file uploading using the native HTML form uopload.
- Resumable file uploading with configurable chunk size.
