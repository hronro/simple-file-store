use std::cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd};

use axum::http::StatusCode;
use sailfish::TemplateOnce;
use time::OffsetDateTime;

use crate::auth::Claims;

#[derive(Default, TemplateOnce)]
#[template(path = "error.stpl")]
pub struct Error<'a> {
    /// HTTP status code of the error.
    pub status: StatusCode,

    /// Title of the page.
    pub title: Option<&'a str>,

    /// Message to display to the user.
    pub message: &'a str,

    /// Display a "Try Again" button when set to true,
    /// otherwise, display a "Go Back" button.
    pub display_try_again_button: bool,
}

#[derive(TemplateOnce)]
#[template(path = "redirect.stpl")]
pub struct Redirect<'a> {
    /// Title of the page.
    pub title: &'a str,

    /// A number of seconds after which to redirect the page.
    /// Default to 3 seconds.
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
impl Default for Redirect<'_> {
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

#[derive(TemplateOnce)]
#[template(path = "home.stpl")]
pub struct Home {
    /// User claims.
    pub claims: Option<Claims>,

    /// The URL of the jump button.
    pub jump_url: &'static str,
}

#[derive(TemplateOnce)]
#[template(path = "files.stpl")]
pub struct Files<'a> {
    /// User claims.
    pub claims: Claims,

    /// The route path prefix.
    pub path_prefix: &'static str,

    /// The matched path.
    pub path: &'a str,

    /// The entries in the current directory.
    pub entries: Vec<FilesEntry>,

    /// The URL to upload files to.
    pub upload_uri: String,
}

// FIXME: For now, Sailfish does not support pattern matching in the
// template. So we have to use struct as a workaround.
//
// #[derive(Eq, PartialEq)]
// pub enum FilesEntry {
//     Dir {
//         name: String,
//         modified: OffsetDateTime,
//     },
//     File {
//         name: String,
//         modified: OffsetDateTime,
//         size: u64,
//     },
// }
// impl Ord for FilesEntry {
//     fn cmp(&self, other: &Self) -> Ordering {
//         match (self, other) {
//             (Self::Dir { name: name1, .. }, Self::Dir { name: name2, .. }) => name1.cmp(name2),
//             (Self::File { name: name1, .. }, Self::File { name: name2, .. }) => name1.cmp(name2),
//             (Self::Dir { .. }, Self::File { .. }) => Ordering::Less,
//             (Self::File { .. }, Self::Dir { .. }) => Ordering::Greater,
//         }
//     }
// }
// impl PartialOrd for FilesEntry {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         Some(self.cmp(other))
//     }
// }
// impl FilesEntry {
//     pub fn name(&self) -> &str {
//         match self {
//             Self::Dir { name, .. } => name,
//             Self::File { name, .. } => name,
//         }
//     }
//
//     pub fn modified(&self) -> OffsetDateTime {
//         match self {
//             Self::Dir { modified, .. } => *modified,
//             Self::File { modified, .. } => *modified,
//         }
//     }
// }

#[derive(Eq, PartialEq)]
pub struct FilesEntry {
    /// The name of the entry.
    pub name: String,

    /// The last modified date of the entry.
    pub modified: OffsetDateTime,

    /// Whether the entry is a directory or not.
    pub is_dir: bool,

    /// File size in bytes. For directories, it's always 0.
    pub size: u64,
}
impl Ord for FilesEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self.is_dir, other.is_dir) {
            (true, true) | (false, false) => self.name.cmp(&other.name),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
        }
    }
}
impl PartialOrd for FilesEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(TemplateOnce)]
#[template(path = "login.stpl")]
pub struct Login<'a> {
    /// The URL to the login form action.
    pub url: &'static str,

    /// The redirect URL after login.
    pub redirect: Option<&'a str>,
}
