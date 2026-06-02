use std::io::{Error as IoError, SeekFrom};

use axum::body::Body;
use axum::extract::{Multipart, OriginalUri, Path};
use axum::http::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse};
use futures::{TryStreamExt, pin_mut};
use sailfish::TemplateOnce;
use time::OffsetDateTime;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt, BufWriter};
use tokio::task::spawn_blocking;
use tokio_util::io::{ReaderStream, StreamReader};

use crate::auth::Claims;
use crate::config::CONFIG;
use crate::errors::ServerError;
use crate::safe_path::safe_join;
use crate::templates;

pub const ROUTE_PATH: &str = "/files/{*file_path}";
pub const ROUTE_PATH_ROOT: &str = "/files";
pub const ROUTE_PATH_ROOT_EMPTY: &str = "/files/";

const READ_BUFFER_SIZE: usize = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum RangeSpec {
    Satisfiable { start: u64, end_inclusive: u64 },
    Unsatisfiable,
    Ignore,
}

fn parse_range(header: Option<&HeaderValue>, file_size: u64) -> RangeSpec {
    let Some(value) = header else {
        return RangeSpec::Ignore;
    };
    let Ok(value) = value.to_str() else {
        return RangeSpec::Ignore;
    };

    let Some((unit, rest)) = value.split_once('=') else {
        return RangeSpec::Ignore;
    };
    if !unit.eq_ignore_ascii_case("bytes") {
        return RangeSpec::Ignore;
    }

    if rest.contains(',') {
        return RangeSpec::Unsatisfiable;
    }

    if file_size == 0 {
        return RangeSpec::Unsatisfiable;
    }

    let Some((start_str, end_str)) = rest.trim().split_once('-') else {
        return RangeSpec::Unsatisfiable;
    };
    let start_str = start_str.trim();
    let end_str = end_str.trim();

    match (start_str.is_empty(), end_str.is_empty()) {
        (true, true) => RangeSpec::Unsatisfiable,
        (true, false) => {
            let Ok(suffix) = end_str.parse::<u64>() else {
                return RangeSpec::Unsatisfiable;
            };
            if suffix == 0 {
                RangeSpec::Unsatisfiable
            } else if suffix >= file_size {
                RangeSpec::Satisfiable {
                    start: 0,
                    end_inclusive: file_size - 1,
                }
            } else {
                RangeSpec::Satisfiable {
                    start: file_size - suffix,
                    end_inclusive: file_size - 1,
                }
            }
        }
        (false, true) => {
            let Ok(start) = start_str.parse::<u64>() else {
                return RangeSpec::Unsatisfiable;
            };
            if start >= file_size {
                RangeSpec::Unsatisfiable
            } else {
                RangeSpec::Satisfiable {
                    start,
                    end_inclusive: file_size - 1,
                }
            }
        }
        (false, false) => {
            let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) else {
                return RangeSpec::Unsatisfiable;
            };
            if end < start || start >= file_size {
                RangeSpec::Unsatisfiable
            } else {
                RangeSpec::Satisfiable {
                    start,
                    end_inclusive: end.min(file_size - 1),
                }
            }
        }
    }
}

pub async fn root_get(
    claims: Claims,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ServerError> {
    get(claims, Path("".to_string()), headers).await
}

pub async fn get(
    claims: Claims,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, ServerError> {
    let full_path = safe_join(&CONFIG.store_path, &path)?;

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
        let file_size = metadata.len();
        let mime = mime_guess::from_path(&full_path)
            .first_or_octet_stream()
            .to_string();

        match parse_range(headers.get(RANGE), file_size) {
            RangeSpec::Ignore => {
                let file = fs::File::open(&full_path).await?;
                Ok((
                    [
                        (CONTENT_TYPE, mime),
                        (CONTENT_LENGTH, file_size.to_string()),
                        (ACCEPT_RANGES, "bytes".to_string()),
                    ],
                    Body::from_stream(ReaderStream::with_capacity(file, READ_BUFFER_SIZE)),
                )
                    .into_response())
            }
            RangeSpec::Unsatisfiable => Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [
                    (CONTENT_RANGE, format!("bytes */{file_size}")),
                    (ACCEPT_RANGES, "bytes".to_string()),
                ],
                Body::empty(),
            )
                .into_response()),
            RangeSpec::Satisfiable {
                start,
                end_inclusive,
            } => {
                let mut file = fs::File::open(&full_path).await?;
                file.seek(SeekFrom::Start(start)).await?;
                let length = end_inclusive - start + 1;
                Ok((
                    StatusCode::PARTIAL_CONTENT,
                    [
                        (CONTENT_TYPE, mime),
                        (CONTENT_LENGTH, length.to_string()),
                        (
                            CONTENT_RANGE,
                            format!("bytes {start}-{end_inclusive}/{file_size}"),
                        ),
                        (ACCEPT_RANGES, "bytes".to_string()),
                    ],
                    Body::from_stream(ReaderStream::with_capacity(
                    file.take(length),
                    READ_BUFFER_SIZE,
                )),
                )
                    .into_response())
            }
        }
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
    let dir_path = safe_join(&CONFIG.store_path, &path)?;

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

    let file_path = safe_join(&dir_path, &format!("{file_name}.form-upload"))?;

    let body_with_io_error = field.map_err(IoError::other);
    let body_reader = StreamReader::new(body_with_io_error);
    pin_mut!(body_reader);
    let mut file = BufWriter::new(fs::File::create(&file_path).await?);

    tokio::io::copy(&mut body_reader, &mut file).await?;

    let final_file_path = safe_join(&dir_path, &file_name)?;

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

#[cfg(test)]
mod tests {
    use super::{RangeSpec, parse_range};
    use axum::http::HeaderValue;

    fn satisfiable(start: u64, end_inclusive: u64) -> RangeSpec {
        RangeSpec::Satisfiable {
            start,
            end_inclusive,
        }
    }

    fn parse(value: &'static str, file_size: u64) -> RangeSpec {
        parse_range(Some(&HeaderValue::from_static(value)), file_size)
    }

    #[test]
    fn no_header_is_ignored() {
        assert_eq!(parse_range(None, 100), RangeSpec::Ignore);
        assert_eq!(parse_range(None, 0), RangeSpec::Ignore);
    }

    #[test]
    fn non_ascii_header_is_ignored() {
        let bytes = [0xff, 0xfe, 0xfd];
        let value = HeaderValue::from_bytes(&bytes).unwrap();
        assert_eq!(parse_range(Some(&value), 100), RangeSpec::Ignore);
    }

    #[test]
    fn missing_equals_is_ignored() {
        assert_eq!(parse("bytes 0-10", 100), RangeSpec::Ignore);
        assert_eq!(parse("garbage", 100), RangeSpec::Ignore);
    }

    #[test]
    fn unknown_unit_is_ignored() {
        assert_eq!(parse("items=0-10", 100), RangeSpec::Ignore);
        assert_eq!(parse("octets=0-10", 100), RangeSpec::Ignore);
    }

    #[test]
    fn unit_is_case_insensitive() {
        assert_eq!(parse("BYTES=0-9", 100), satisfiable(0, 9));
        assert_eq!(parse("Bytes=0-9", 100), satisfiable(0, 9));
        assert_eq!(parse("bYtEs=0-9", 100), satisfiable(0, 9));
    }

    #[test]
    fn standard_start_end_range() {
        assert_eq!(parse("bytes=0-99", 1000), satisfiable(0, 99));
        assert_eq!(parse("bytes=100-199", 1000), satisfiable(100, 199));
        assert_eq!(parse("bytes=0-0", 1000), satisfiable(0, 0));
    }

    #[test]
    fn end_is_clamped_to_file_size() {
        assert_eq!(parse("bytes=0-99999", 1000), satisfiable(0, 999));
        assert_eq!(parse("bytes=500-99999", 1000), satisfiable(500, 999));
    }

    #[test]
    fn open_ended_range() {
        assert_eq!(parse("bytes=0-", 1000), satisfiable(0, 999));
        assert_eq!(parse("bytes=500-", 1000), satisfiable(500, 999));
        assert_eq!(parse("bytes=999-", 1000), satisfiable(999, 999));
    }

    #[test]
    fn suffix_range() {
        assert_eq!(parse("bytes=-100", 1000), satisfiable(900, 999));
        assert_eq!(parse("bytes=-1", 1000), satisfiable(999, 999));
    }

    #[test]
    fn suffix_larger_than_file_clamps_to_whole_file() {
        assert_eq!(parse("bytes=-99999", 1000), satisfiable(0, 999));
        assert_eq!(parse("bytes=-1000", 1000), satisfiable(0, 999));
    }

    #[test]
    fn zero_suffix_is_unsatisfiable() {
        assert_eq!(parse("bytes=-0", 1000), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn start_at_or_past_file_size_is_unsatisfiable() {
        assert_eq!(parse("bytes=1000-1500", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=1500-2000", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=1000-", 1000), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn end_before_start_is_unsatisfiable() {
        assert_eq!(parse("bytes=100-50", 1000), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn both_sides_empty_is_unsatisfiable() {
        assert_eq!(parse("bytes=-", 1000), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn missing_dash_is_unsatisfiable() {
        assert_eq!(parse("bytes=100", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=", 1000), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn non_numeric_components_are_unsatisfiable() {
        assert_eq!(parse("bytes=foo-bar", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=10-foo", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=foo-10", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=-foo", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=-10.5", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=-1", 0), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn negative_numbers_are_unsatisfiable() {
        assert_eq!(parse("bytes=-10-20", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=10--20", 1000), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn multi_range_is_unsatisfiable() {
        assert_eq!(parse("bytes=0-100,200-300", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=0-100,", 1000), RangeSpec::Unsatisfiable);
        assert_eq!(
            parse("bytes=0-100,200-300,400-500", 1000),
            RangeSpec::Unsatisfiable
        );
    }

    #[test]
    fn empty_file_with_any_range_is_unsatisfiable() {
        assert_eq!(parse("bytes=0-0", 0), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=0-", 0), RangeSpec::Unsatisfiable);
        assert_eq!(parse("bytes=-100", 0), RangeSpec::Unsatisfiable);
    }

    #[test]
    fn whitespace_around_segment_is_trimmed() {
        assert_eq!(parse("bytes= 0-99 ", 1000), satisfiable(0, 99));
        assert_eq!(parse("bytes= 500- ", 1000), satisfiable(500, 999));
        assert_eq!(parse("bytes= -100 ", 1000), satisfiable(900, 999));
        assert_eq!(parse("bytes=0 - 99", 1000), satisfiable(0, 99));
    }

    #[test]
    fn single_byte_file_boundary() {
        assert_eq!(parse("bytes=0-0", 1), satisfiable(0, 0));
        assert_eq!(parse("bytes=0-", 1), satisfiable(0, 0));
        assert_eq!(parse("bytes=-1", 1), satisfiable(0, 0));
        assert_eq!(parse("bytes=1-1", 1), RangeSpec::Unsatisfiable);
    }
}
