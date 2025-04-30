/// Configuration for the HTML redirect page.
pub struct HtmlRedirectConfig<'a> {
    /// Title of the HTML page.
    pub title: &'a str,

    /// A number of seconds after which to refresh the page.
    pub time: Option<u8>,

    /// The URL to redirect to.
    pub url: &'a str,

    /// Treat the message as a success message or not.
    pub success: bool,

    /// The icon to display to the user.
    pub icon: Option<&'a str>,

    /// An optional message to display to the user.
    pub message: &'a str,
}
impl Default for HtmlRedirectConfig<'_> {
    fn default() -> Self {
        Self {
            title: "Redirecting",
            time: None,
            url: "/",
            success: true,
            icon: None,
            message: "",
        }
    }
}

/// Generate a HTML redirect page that contains a meta refresh tag.
pub fn gen_html_redirect(config: HtmlRedirectConfig) -> String {
    let HtmlRedirectConfig {
        title,
        url,
        success,
        ..
    } = config;
    let time = config.time.unwrap_or(3);
    let icon = config.icon.unwrap_or(if success { "✓" } else { "✕" });
    let message = config.message;

    let status = if success { "success" } else { "error" };

    format!(
        r###"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<meta http-equiv="refresh" content="{time};url={url}">
<title>{title}</title>
<link rel="stylesheet" href="/_assets/reset.css">
<link rel="stylesheet" href="/_assets/redirect.css">
<style>
	.redirect-progress::after {{
		animation: progress {time}s linear forwards;
	}}
</style>
</head>
<body>
<div class="circles">
	<div class="circle circle-1"></div>
	<div class="circle circle-2"></div>
</div>

<main class="redirect-container">
	<div class="redirect-card">
		<div class="status-message {status}">
			<div class="status-icon">{icon}</div>
			<h1>{title}</h1>
			<p>{message}</p>
		</div>

		<div class="redirect-progress"></div>

		<a autofocus href="{url}" class="redirect-link">Continue</a>

		<div class="redirect-footer">
			Redirecting automatically in {time} seconds...
		</div>
	</div>
</main>
</body>
</html>"###,
    )
}
