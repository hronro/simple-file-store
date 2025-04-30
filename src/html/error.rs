use axum::http::StatusCode;

/// Configuration for the HTML error page.
#[derive(Default)]
pub struct HtmlErrorConfig<'a> {
    /// Status code of the error.
    pub status: StatusCode,

    /// Title of the page.
    pub title: Option<&'a str>,

    /// Message to display to the user.
    pub message: &'a str,

    /// Display a "Try Again" button when setting to true,
    /// otherwise, display a "Go Back" button.
    pub display_try_again_button: bool,
}

pub fn gen_html_error(config: HtmlErrorConfig) -> String {
    let HtmlErrorConfig {
        message,
        display_try_again_button,
        ..
    } = config;

    let status = config.status.as_u16();
    let title = config
        .title
        .unwrap_or(config.status.canonical_reason().unwrap_or("Error"));

    format!(
        r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<link rel="stylesheet" href="/_assets/reset.css">
<link rel="stylesheet" href="/_assets/error.css">
</head>
<body>
<div class="circles">
	<div class="circle circle-1"></div>
	<div class="circle circle-2"></div>
</div>

<main class="main-container">
	<div class="error-card">
		<div class="error-header">
			<div class="error-pattern"></div>
			<div class="error-code">{status}</div>
			<div class="error-title">{title}</div>
		</div>

		<div class="error-body">
			<div class="error-message">{message}</div>

			<div class="actions">
				<a href="/" class="btn btn-primary">Go to Homepage</a>
				{}
				
			</div>
		</div>
	</div>
</main>
</body>
</html>"###,
        if display_try_again_button {
            "<a href=\"#\" onclick=\"window.location.reload()\" class=\"btn btn-secondary\">Try Again</a>"
        } else {
            "<a href=\"javascript:history.back()\" class=\"btn btn-secondary\">Go Back</a>"
        }
    )
}

pub fn not_found<T: AsRef<str>>(path: Option<T>) -> String {
    if let Some(path) = path {
        gen_html_error(HtmlErrorConfig {
			status: StatusCode::NOT_FOUND,
			title: Some("Page Not Found"),
			message: format!(
				"The page you are looking for doesn't exist or has been moved. The requested path is: <b>{}</b>",
				path.as_ref()
			)
			.as_str(),
			..Default::default()
		})
    } else {
        gen_html_error(HtmlErrorConfig {
            status: StatusCode::NOT_FOUND,
            title: Some("Page Not Found"),
            message: "The page you are looking for doesn't exist or has been moved.",
            ..Default::default()
        })
    }
}

pub fn internal_server_error<T: AsRef<str>>(message: T) -> String {
    gen_html_error(HtmlErrorConfig {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: message.as_ref(),
        display_try_again_button: true,
        ..Default::default()
    })
}

pub fn bad_request<T: AsRef<str>>(message: T) -> String {
    gen_html_error(HtmlErrorConfig {
        status: StatusCode::BAD_REQUEST,
        title: Some("Bad Request"),
        message: message.as_ref(),
        ..Default::default()
    })
}
