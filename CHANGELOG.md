# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Content-Type` HTTP header for user uploaded files, guessing the MIME type based on the file extension.
- HTTP cache for built-in front-end assets.
- time counter for file upload.
- TLS support.

### Changed

- Split the file link and the download link. Previously whether you click the file itself or click the download button, it would always download the file. Now, clicking the file link will open the file in a new tab, while clicking the download button will download the file.
- Upgrade Rust to v1.87

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
