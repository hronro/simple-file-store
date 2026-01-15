# Simple File Store

A lightweight, high-performance file storage server.

## Features

- **Blazing Fast Performance**: Optimized for speed with small binary size
- **Extremely Lightweight**: Minimal resource footprint with optimized release profile
- **No JavaScript Required**: Pure HTML/CSS frontend works in any browser without JavaScript
  - When JavaScript is enabled, enhanced features are available:
    - Upload progress display
    - Resumable uploading support
- **Simple Authentication**: Basic username/password authentication with JWT tokens
- **Configurable**: Easily configure via environment variables or command-line arguments
- **Resumable File Uploads**: Support for large file uploads with configurable chunk size
- **TLS Support**: Built-in TLS support, without depending system TLS libraries like OpenSSL

## Screenshots

![Screenshot 0](https://raw.githubusercontent.com/hronro/simple-file-store/refs/heads/media/screenshots/Screenshot%200.avif)
![Screenshot 1](https://raw.githubusercontent.com/hronro/simple-file-store/refs/heads/media/screenshots/Screenshot%201.avif)

## Getting Started

### Installation

Download the pre-compiled binary from [GitHub Releases Page](https://github.com/hronro/simple-file-store/releases), or compile it yourself:

```sh
# Clone the repository
git clone https://github.com/hronro/simple-file-store.git
cd simple-file-store

# Build the release version
cargo build --release

# The binary will be located in the target/release directory
```

The run the server:

```sh
./simple-file-store
```

By default, the server will:
- Listen on `[::]:8080` (IPv6 and IPv4) with HTTP protocol (HTTPS is disabled by default)
- Store files in the current directory
- Use "admin" as the default username and "password" as the default password

### Configuration

Configure Simple File Store using environment variables or command-line arguments:

| Environment Variable | Command-line Flag | Description | Default |
|---------------------|-------------------|-------------|---------|
| `SFS_LISTEN`        | `--listen`, `-l`  | Listen address | `[::]:8080` |
| `SFS_STORE_PATH`    | `--store-path`, `-p` | Path to store files | Current directory |
| `SFS_CHUNK_SIZE`    | `--chunk-size`, `-s` | Chunk size in bytes | 8MB |
| `SFS_USERNAME`      | `--username`, `-u` | Username for authentication | `admin` |
| `SFS_PASSWORD`      | `--password`, `-w` | Password for authentication | `password` |
| `SFS_SECRET`        | `--secret`, `-x` | Secret for JWT | Random 16 characters |
| `SFS_TOKEN_EXP`     | `--token-exp`, `-e` | Token expiry in seconds | 24 hours (86400) |
| `SFS_TLS_CERT`      | `--tls-cert`, `-c` | Path to TLS certificate file | None (HTTP only) |
| `SFS_TLS_KEY`       | `--tls-key`, `-k`  | Path to TLS private key file | None (HTTP only) |

Example:

```bash
# Using environment variables
SFS_LISTEN=127.0.0.1:3000 SFS_USERNAME=user SFS_PASSWORD=secure ./simple-file-store

# Using command-line arguments
./simple-file-store --listen 127.0.0.1:3000 --username user --password secure
```

## Why Simple File Store?

### Lightweight and Efficient
Simple File Store is designed for self hosting, with a focus on speed and efficiency. It has a small binary size and minimal resource requirements, making it ideal for low-powered devices or environments where performance is critical.

### Browser Compatibility
- Works in any browser without JavaScript requirements
- When JavaScript is enabled, offers enhanced user experience with progress indicators and resumable uploads
- Compatible with older browsers, text-based browsers, and low-powered devices

### Security
- JWT-based authentication
- Configurable token expiration
- Custom secret key support

## License

[AGPL-3.0 License](LICENSE)

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

The commit message should follow the [gitmoji](https://gitmoji.dev) format, and all changes should be documented in the [CHANGELOG.md](CHANGELOG.md) file.
