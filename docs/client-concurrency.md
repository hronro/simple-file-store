# Client Upload Concurrency

Simple File Store supports resumable uploads by splitting a file into fixed-size chunks and uploading each chunk with a `PUT /upload/{path}` request.

## Built-in Web Client

The built-in web client uploads resumable chunks with a concurrency of `6`.

This matches the common browser behavior where HTTP/1.1 connections to one origin are limited to around 6 concurrent TCP connections. It is a conservative default that works well for browser users and avoids opening more chunk requests than most browsers can actually run in parallel.

The browser client does not currently auto-tune this value. It starts up to 6 chunk uploads, then starts another chunk each time one finishes.

## Server Limits

The server also enforces upload concurrency limits. These limits protect small servers and keep native or third-party clients from overloading the process, disk, or network stack.

The limits are configured with these options:

| Environment Variable | Command-line Flag | Default | Description |
| --- | --- | ---: | --- |
| `SFS_MAX_ACTIVE_UPLOAD_CHUNKS` | `--max-active-upload-chunks` | `32` | Maximum active resumable chunk requests across the whole server. |
| `SFS_MAX_ACTIVE_CHUNKS_PER_UPLOAD` | `--max-active-chunks-per-upload` | `6` | Maximum active resumable chunk requests for one target file. |
| `SFS_MAX_ACTIVE_UPLOAD_BYTES` | `--max-active-upload-bytes` | `536870912` | Active upload byte budget, based on each active chunk's declared `Content-Length`. |

If a limit is reached, the server responds with `429 Too Many Requests` and a `Retry-After: 1` header. Clients should retry the chunk later instead of treating the whole upload as permanently failed.

The byte budget is not a memory allocation target. After streaming upload bodies to disk, the server does not keep each whole chunk in memory. The byte budget is a backpressure mechanism that limits how much upload work can be active at once.

## Native And Third-party Clients

Native clients may use higher concurrency than the built-in web client, but they should treat concurrency as adaptive rather than fixed.

Recommended behavior:

1. Start with a moderate concurrency such as `4` or `6`.
2. Increase gradually while throughput improves.
3. Back off when the server returns `429 Too Many Requests`.
4. Respect the `Retry-After` response header.
5. Retry individual chunks instead of restarting the entire upload.
6. Keep a configurable maximum concurrency for users deploying to small servers.

Avoid very high default concurrency. More parallel chunk requests are not always faster. Once the server or disk is saturated, extra concurrency can increase disk contention, dirty page cache pressure, TLS CPU cost, and metadata lock contention.

For small servers, use values like:

```sh
SFS_MAX_ACTIVE_UPLOAD_CHUNKS=4
SFS_MAX_ACTIVE_CHUNKS_PER_UPLOAD=2
SFS_MAX_ACTIVE_UPLOAD_BYTES=268435456
```

For larger servers with native clients, use values like:

```sh
SFS_MAX_ACTIVE_UPLOAD_CHUNKS=32
SFS_MAX_ACTIVE_CHUNKS_PER_UPLOAD=16
SFS_MAX_ACTIVE_UPLOAD_BYTES=2147483648
```

Clients should not assume the server accepts the client's desired concurrency. The server limits are authoritative.
