mod account;
pub mod accounts;
mod cache;
pub mod dav;
mod edits;
mod favorites;
mod files;
pub mod json;
mod login;
mod net;
mod ops;
mod settings;
pub mod store;
mod transfer;
mod undo;
mod upload;

use dav::Dav;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub use account::Account;
pub use edits::Saved;
pub use favorites::Favorite;
pub use files::File;
pub use login::{Credentials, LoginFlow, login_flow_poll, login_flow_start, password_login};
pub use ops::valid_name;
pub use settings::DEVICE_ONLY;
pub use transfer::Progress;
pub use upload::Taken;

pub trait ChangeListener: Send + Sync {
    /// Settings changed by another device arrived.
    fn settings_changed(&self) {}
}

pub struct Client {
    db: Mutex<Connection>,
    /// The connections of the accounts connected in this run, by account id.
    dav: Mutex<HashMap<i64, Arc<Dav>>>,
    listeners: Mutex<Vec<Box<dyn ChangeListener>>>,
    journal: Mutex<undo::Journal>,
    /// What can be made again from the server: the copies files are opened from (`transfer.rs`).
    cache_dir: std::path::PathBuf,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Client {
    pub fn open(data_dir: String) -> Result<Client> {
        std::fs::create_dir_all(&data_dir).map_err(|e| Error::Db(e.to_string()))?;
        let db = store::open(&format!("{data_dir}/foldav.sqlite"))?;
        Ok(Client {
            db: Mutex::new(db),
            dav: Mutex::default(),
            listeners: Mutex::new(Vec::new()),
            journal: Mutex::default(),
            cache_dir: std::path::Path::new(&data_dir).join("cache"),
        })
    }

    /// Keep the copies of files in `dir` rather than in a folder of the data (a front-end's
    /// cache folder, which the system may clean).
    pub fn cache_in(mut self, dir: impl Into<std::path::PathBuf>) -> Client {
        self.cache_dir = dir.into();
        self
    }

    pub fn subscribe(&self, listener: Box<dyn ChangeListener>) {
        self.listeners.lock().unwrap().push(listener);
    }
}

#[derive(Debug)]
pub enum Error {
    Auth,
    Conflict,
    NoAccount,
    NotFound(String),
    /// Something has that name already.
    Exists(String),
    /// Not a name a file or folder can have.
    Invalid(String),
    /// Locked on the server (HTTP 423), by an editor that has it open.
    Locked(String),
    Http(u16, String),
    Net(String),
    /// Reading or writing a file of this device failed.
    Io(String),
    /// Stopped by `Progress::cancel`.
    Cancelled,
    Parse(String),
    Db(String),
    /// The data was left by a newer Foldav, whose schema this one may not read or write.
    Newer(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Auth => write!(f, "authentication failed"),
            Error::Conflict => write!(f, "precondition failed (changed on server)"),
            Error::NoAccount => write!(f, "no account configured"),
            Error::NotFound(s) => write!(f, "not found: {s}"),
            Error::Exists(s) => write!(f, "exists already: {s}"),
            Error::Invalid(s) => write!(f, "invalid name: {s}"),
            Error::Locked(s) => write!(f, "locked on the server: {s}"),
            // A WebDAV error body is XML: its <s:message> is the part worth reading, else (it
            // can be empty) the exception's name. A web server's or proxy's error page is
            // HTML: its text only.
            Error::Http(code, body) => {
                let tag = |t: &str| {
                    body.split_once(&format!("<s:{t}>"))
                        .and_then(|(_, m)| m.split_once(&format!("</s:{t}>")))
                        .map(|(m, _)| m.trim())
                        .filter(|m| !m.is_empty())
                };
                let message = tag("message")
                    .or_else(|| tag("exception").map(|e| e.rsplit('\\').next().unwrap_or(e)))
                    .map_or_else(|| html_text(body), String::from);
                let short: String = message.chars().take(300).collect();
                write!(f, "HTTP {code}: {}", short.trim())
            }
            Error::Net(s) => write!(f, "network: {s}"),
            Error::Io(s) => write!(f, "file: {s}"),
            Error::Cancelled => write!(f, "cancelled"),
            Error::Parse(s) => write!(f, "parse: {s}"),
            Error::Db(s) => write!(f, "database: {s}"),
            Error::Newer(s) => write!(f, "made by a newer version of Foldav: {s}"),
        }
    }
}

impl std::error::Error for Error {}

/// The text of an HTML page (its body's, without scripts and styles), on one line; anything else
/// as it is.
fn html_text(body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    if !lower.trim_start().starts_with("<!doctype html") && !lower.contains("<html") {
        return body.into();
    }
    let mut text = String::new();
    let mut rest = body;
    let mut lower = lower.as_str();
    while let Some(open) = lower.find('<') {
        text.push_str(&rest[..open]);
        // Skipped with their content.
        let end = ["head", "script", "style"]
            .iter()
            .find(|t| {
                lower[open + 1..].starts_with(*t)
                    && !lower[open + 1 + t.len()..].starts_with(|c: char| c.is_ascii_alphanumeric())
            })
            .and_then(|t| lower[open..].find(&format!("</{t}")).map(|i| open + i))
            .unwrap_or(open);
        let Some(close) = lower[end..].find('>') else {
            rest = "";
            break;
        };
        text.push(' ');
        rest = &rest[end + close + 1..];
        lower = &lower[end + close + 1..];
    }
    text.push_str(rest);
    let text = [
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#39;", "'"),
        ("&#x27;", "'"),
        ("&nbsp;", " "),
        ("&amp;", "&"),
    ]
    .iter()
    .fold(text, |t, (entity, c)| t.replace(entity, c));
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Base64; `url` = URL-safe alphabet without padding.
pub fn b64encode(data: &[u8], url: bool) -> String {
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..=c.len() {
            let ch = B64[(n >> (18 - 6 * i) & 63) as usize] as char;
            out.push(match (url, ch) {
                (true, '+') => '-',
                (true, '/') => '_',
                _ => ch,
            });
        }
        if !url {
            out.extend(std::iter::repeat_n('=', 3 - c.len()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_errors_show_the_server_message_or_exception() {
        let body = |inner: &str| {
            format!(
                "<?xml version=\"1.0\"?>\n<d:error xmlns:d=\"DAV:\" xmlns:s=\"http://sabredav.org/ns\">{inner}</d:error>"
            )
        };
        let shown = |inner: &str| Error::Http(423, body(inner)).to_string();
        assert_eq!(
            shown("<s:exception>Sabre\\DAV\\Exception\\Locked</s:exception><s:message/>"),
            "HTTP 423: Locked"
        );
        assert_eq!(
            shown("<s:exception>X</s:exception><s:message>Report.odt is locked</s:message>"),
            "HTTP 423: Report.odt is locked"
        );
        assert_eq!(
            Error::Http(500, "oops".into()).to_string(),
            "HTTP 500: oops"
        );
    }

    #[test]
    fn http_errors_show_an_html_pages_text() {
        let nginx = "<html>\r\n<head><title>502 Bad Gateway</title></head>\r\n<body>\r\n<center><h1>502 Bad Gateway</h1></center>\r\n<hr><center>nginx</center>\r\n</body>\r\n</html>\r\n";
        assert_eq!(
            Error::Http(502, nginx.into()).to_string(),
            "HTTP 502: 502 Bad Gateway nginx"
        );
        let python = r#"<!DOCTYPE HTML>
<html lang="en">
    <head>
        <meta charset="utf-8">
        <style type="text/css">
            :root { color-scheme: light dark; }
        </style>
        <title>Error response</title>
    </head>
    <body>
        <h1>Error response</h1>
        <p>Error code: 501</p>
        <p>Message: Unsupported method (&#x27;PROPFIND&#x27;).</p>
        <script>let a = 1 < 2;</script>
    </body>
</html>
"#;
        assert_eq!(
            Error::Http(501, python.into()).to_string(),
            "HTTP 501: Error response Error code: 501 Message: Unsupported method ('PROPFIND')."
        );
    }
}
