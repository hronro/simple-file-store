# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Use the official `sailfish` crate instead of the forked version, since the official `sailfish` crate now supports removing newlines in the rendered HTML files.

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
