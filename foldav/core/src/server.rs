//! What the server of an account offers (its capabilities, so the front-end shows only what
//! works there) and what the account uses of its quota.

use crate::dav::DAV;
use crate::json::Json;
use crate::{Client, Result};

/// What Nextcloud Files features the server has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Features {
    /// A deleted file goes to the trash bin, from where it comes back.
    pub trash: bool,
    /// The server keeps the versions of a file.
    pub versions: bool,
    /// Sharing by link and with users (the OCS `files_sharing` API).
    pub sharing: bool,
    /// Tags on files (`systemtags`).
    pub tags: bool,
}

fn flag(j: &Json, path: &[&str]) -> bool {
    path.iter()
        .try_fold(j, |j, k| j.get(k))
        .is_some_and(|v| matches!(v, Json::Bool(true)))
}

/// `ocs.data` of `cloud/capabilities` as features.
fn features(data: &Json) -> Features {
    let caps = |path: &[&str]| flag(data, &[&["capabilities"][..], path].concat());
    Features {
        trash: caps(&["files", "undelete"]),
        versions: caps(&["files", "versioning"]),
        sharing: caps(&["files_sharing", "api_enabled"]),
        tags: caps(&["systemtags", "enabled"]),
    }
}

/// What an account has used, and what it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    /// Bytes.
    pub used: u64,
    /// Bytes in all; None when there is no limit (or the server does not say).
    pub total: Option<u64>,
}

impl Client {
    /// What the server of account `id` offers.
    pub fn features(&self, id: i64) -> Result<Features> {
        Ok(features(&self.dav(id)?.ocs(
            "GET",
            "cloud/capabilities",
            "",
        )?))
    }

    /// The storage account `id` uses (`d:quota-used-bytes` and `d:quota-available-bytes` of the
    /// Files root).
    pub fn quota(&self, id: i64) -> Result<Quota> {
        let home = self.files_home(id)?;
        let props = "<d:quota-used-bytes/><d:quota-available-bytes/>";
        let r = self.dav(id)?.propfind(&home, "0", props)?;
        let num = |n: &str| {
            r.first()
                .and_then(|r| r.props.find_text(DAV, n))
                .and_then(|v| v.parse::<i64>().ok())
        };
        let used = num("quota-used-bytes").unwrap_or(0).max(0) as u64;
        // Negative: unlimited, or not known.
        let total = num("quota-available-bytes")
            .filter(|a| *a >= 0)
            .map(|a| used + a as u64);
        Ok(Quota { used, total })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_from_the_capabilities() {
        let j = Json::parse(
            r#"{"capabilities":{"files":{"undelete":true,"versioning":false},"files_sharing":{"api_enabled":true}}}"#,
        )
        .unwrap();
        assert_eq!(
            features(&j),
            Features {
                trash: true,
                versions: false,
                sharing: true,
                tags: false
            }
        );
    }
}
