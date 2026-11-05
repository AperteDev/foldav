//! Sharing files and folders, through Nextcloud's OCS `files_sharing` API: public links (with a
//! password and an expiry date), shares with users and groups, and who can be shared with.

use crate::dav::form_encode;
use crate::json::Json;
use crate::{Client, Result};

const API: &str = "apps/files_sharing/api/v1";

/// The permissions of a share (they add up).
pub mod perm {
    pub const READ: u32 = 1;
    pub const UPDATE: u32 = 2;
    pub const CREATE: u32 = 4;
    pub const DELETE: u32 = 8;
    pub const SHARE: u32 = 16;
    /// Everything a file or folder can be given.
    pub const ALL: u32 = 31;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareKind {
    User,
    Group,
    /// A public link.
    Link,
    /// A link sent to an email address.
    Email,
    Other(i64),
}

impl ShareKind {
    fn of(n: i64) -> ShareKind {
        match n {
            0 => ShareKind::User,
            1 => ShareKind::Group,
            3 => ShareKind::Link,
            4 => ShareKind::Email,
            n => ShareKind::Other(n),
        }
    }

    fn number(self) -> i64 {
        match self {
            ShareKind::User => 0,
            ShareKind::Group => 1,
            ShareKind::Link => 3,
            ShareKind::Email => 4,
            ShareKind::Other(n) => n,
        }
    }
}

/// One share of a file or folder.
#[derive(Debug, Clone, PartialEq)]
pub struct ShareInfo {
    pub id: String,
    pub kind: ShareKind,
    /// The user's or group's id; for a link, empty.
    pub with: String,
    /// The name to show for it.
    pub label: String,
    /// The public link, for a link.
    pub url: Option<String>,
    pub permissions: u32,
    pub has_password: bool,
    /// "2026-12-31", when the share ends.
    pub expires: Option<String>,
}

fn share(j: &Json) -> ShareInfo {
    let text = |k| j.get(k).and_then(Json::str).unwrap_or_default().to_string();
    let num = |k| j.get(k).and_then(Json::num).unwrap_or(0.0) as i64;
    let kind = ShareKind::of(num("share_type"));
    let named = [text("share_with_displayname"), text("share_with")]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_default();
    ShareInfo {
        // The id is a string in the answer, a number in some versions.
        id: match j.get("id") {
            Some(Json::Num(n)) => (*n as i64).to_string(),
            _ => text("id"),
        },
        kind,
        with: text("share_with"),
        label: match kind {
            ShareKind::Link => Some(text("label"))
                .filter(|l| !l.is_empty())
                .unwrap_or_default(),
            _ => named,
        },
        url: Some(text("url")).filter(|u| !u.is_empty()),
        permissions: num("permissions") as u32,
        has_password: j
            .get("password")
            .and_then(Json::str)
            .is_some_and(|p| !p.is_empty()),
        expires: j
            .get("expiration")
            .and_then(Json::str)
            .map(|d| d.split(' ').next().unwrap_or(d).to_string()),
    }
}

/// A user or group a file can be shared with.
#[derive(Debug, Clone, PartialEq)]
pub struct Sharee {
    pub kind: ShareKind,
    pub id: String,
    pub name: String,
}

impl Client {
    /// The shares of the file or folder at `path` (users, groups and links).
    pub fn file_shares(&self, id: i64, path: &str) -> Result<Vec<ShareInfo>> {
        let q = format!("{API}/shares?path=%2F{}&reshares=true", form_encode(path));
        let data = self.dav(id)?.ocs("GET", &q, "")?;
        Ok(data.arr().iter().map(share).collect())
    }

    /// Share `path` with a user or group (`kind`), `permissions` as `perm`.
    pub fn share_with(
        &self,
        id: i64,
        path: &str,
        kind: ShareKind,
        with: &str,
        permissions: u32,
    ) -> Result<ShareInfo> {
        let form = format!(
            "path=%2F{}&shareType={}&shareWith={}&permissions={permissions}",
            form_encode(path),
            kind.number(),
            form_encode(with)
        );
        Ok(share(&self.dav(id)?.ocs(
            "POST",
            &format!("{API}/shares"),
            &form,
        )?))
    }

    /// Make a public link to `path`; read-only unless `edit` (a folder's link then lets
    /// anyone with it upload, change and delete).
    pub fn share_link(
        &self,
        id: i64,
        path: &str,
        edit: bool,
        password: Option<&str>,
        expires: Option<&str>,
    ) -> Result<ShareInfo> {
        let permissions = if edit {
            perm::ALL & !perm::SHARE
        } else {
            perm::READ
        };
        let mut form = format!(
            "path=%2F{}&shareType=3&permissions={permissions}",
            form_encode(path)
        );
        for (k, v) in [("password", password), ("expireDate", expires)] {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                form.push_str(&format!("&{k}={}", form_encode(v)));
            }
        }
        Ok(share(&self.dav(id)?.ocs(
            "POST",
            &format!("{API}/shares"),
            &form,
        )?))
    }

    /// Change a share: its permissions, and for a link its password (empty: none) and expiry
    /// ("2026-12-31", empty: none); what is None stays.
    pub fn update_share(
        &self,
        id: i64,
        share: &str,
        permissions: Option<u32>,
        password: Option<&str>,
        expires: Option<&str>,
    ) -> Result<ShareInfo> {
        let mut form = Vec::new();
        if let Some(p) = permissions {
            form.push(format!("permissions={p}"));
        }
        for (k, v) in [("password", password), ("expireDate", expires)] {
            if let Some(v) = v {
                form.push(format!("{k}={}", form_encode(v)));
            }
        }
        let data = self.dav(id)?.ocs(
            "PUT",
            &format!("{API}/shares/{}", form_encode(share)),
            &form.join("&"),
        )?;
        Ok(self::share(&data))
    }

    /// Stop sharing: delete share `share`.
    pub fn delete_share(&self, id: i64, share: &str) -> Result<()> {
        self.dav(id)?
            .ocs(
                "DELETE",
                &format!("{API}/shares/{}", form_encode(share)),
                "",
            )
            .map(|_| ())
    }

    /// The users and groups matching `query`, to share with.
    pub fn find_sharees(&self, id: i64, query: &str) -> Result<Vec<Sharee>> {
        let q = format!(
            "apps/files_sharing/api/v1/sharees?search={}&itemType=file&perPage=20",
            form_encode(query)
        );
        let data = self.dav(id)?.ocs("GET", &q, "")?;
        let mut found = Vec::new();
        let mut add = |list: Option<&Json>, kind: ShareKind| {
            for s in list.map(Json::arr).unwrap_or_default() {
                let (Some(value), label) = (s.get("value"), s.get("label")) else {
                    continue;
                };
                let id = value
                    .get("shareWith")
                    .and_then(Json::str)
                    .unwrap_or_default()
                    .to_string();
                let name = label.and_then(Json::str).unwrap_or(&id).to_string();
                if !id.is_empty() && !found.iter().any(|f: &Sharee| f.id == id && f.kind == kind) {
                    found.push(Sharee { kind, id, name });
                }
            }
        };
        let exact = data.get("exact");
        add(exact.and_then(|e| e.get("users")), ShareKind::User);
        add(exact.and_then(|e| e.get("groups")), ShareKind::Group);
        add(data.get("users"), ShareKind::User);
        add(data.get("groups"), ShareKind::Group);
        Ok(found)
    }
}
