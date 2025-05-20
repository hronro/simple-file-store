use axum::extract::Path;
use axum::http::{
    StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE},
};
use axum::response::IntoResponse;

pub const ROUTE_PATH: &str = "/_assets/{asset}";

const CACHE_CONTROL_VALUE: &str = "max-age=604800"; // Cache for 1 week

pub async fn get(Path(path): Path<String>) -> impl IntoResponse {
    /// Generate a response based on the file type.
    macro_rules! file_matches {
        ($(($type:tt, $path:expr),)*) => {
            match path.as_str() {
                $(
                    $path => __file_matches_content_type!($type, $path),
                )*
                _ => (StatusCode::NOT_FOUND, ()).into_response(),
            }
        };
    }
    macro_rules! __file_matches_content_type {
        ("css", $path:expr) => {
            (
                StatusCode::OK,
                [
                    (CONTENT_TYPE, "text/css; charset=utf-8"),
                    (CACHE_CONTROL, CACHE_CONTROL_VALUE),
                ],
                include_str!($path),
            )
                .into_response()
        };

        ("js", $path:expr) => {
            (
                StatusCode::OK,
                [
                    (CONTENT_TYPE, "text/javascript; charset=utf-8"),
                    (CACHE_CONTROL, CACHE_CONTROL_VALUE),
                ],
                include_str!($path),
            )
                .into_response()
        };

        ($other:tt, $path:expr) => {
            compile_error!(concat!(
                "Unsupported file type: ",
                $other,
                ". Only 'css' and 'js' are supported."
            ))
        };
    }

    file_matches!(
        ("css", "error.css"),
        ("css", "files.css"),
        ("css", "index.css"),
        ("css", "login.css"),
        ("css", "redirect.css"),
        ("css", "reset.css"),
        ("js", "upload.js"),
    )
}
