use axum::http::StatusCode;
use sailfish::TemplateOnce;

use crate::templates;

pub fn not_found<T: AsRef<str>>(path: Option<T>) -> String {
    if let Some(path) = path {
        templates::Error {
			status: StatusCode::NOT_FOUND,
			title: Some("Page Not Found"),
			message: format!(
				"The page you are looking for doesn't exist or has been moved. The requested path is: <b>{}</b>",
				path.as_ref()
			)
			.as_str(),
			..Default::default()
		}.render_once().unwrap()
    } else {
        templates::Error {
            status: StatusCode::NOT_FOUND,
            title: Some("Page Not Found"),
            message: "The page you are looking for doesn't exist or has been moved.",
            ..Default::default()
        }
        .render_once()
        .unwrap()
    }
}

pub fn internal_server_error<T: AsRef<str>>(message: T) -> String {
    templates::Error {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: message.as_ref(),
        display_try_again_button: true,
        ..Default::default()
    }
    .render_once()
    .unwrap()
}

pub fn bad_request<T: AsRef<str>>(message: T) -> String {
    templates::Error {
        status: StatusCode::BAD_REQUEST,
        title: Some("Bad Request"),
        message: message.as_ref(),
        ..Default::default()
    }
    .render_once()
    .unwrap()
}
