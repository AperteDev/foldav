//! The versions of a file the server keeps (Nextcloud's `versions` DAV root): listing them,
//! putting one back, and fetching one to open.

use crate::dav::{DAV, http_date};
use crate::{Client, Error, File, Progress, Result};
use std::path::{Path, PathBuf};

/// An earlier version of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    /// What the server calls it (a time).
    pub id: String,
    /// Unix time it was the file.
    pub modified: i64,
    pub size: u64,
}

impl Client {
    /// `/remote.php/dav/versions/<user>/`.
    fn versions_home(&self, id: i64) -> Result<String> {
        Ok(self.files_home(id)?.replacen("/files/", "/versions/", 1))
    }

    /// The earlier versions of `file`, the latest first (the file itself is not among them).
    pub fn versions(&self, id: i64, file: &File) -> Result<Vec<Version>> {
        let home = self.versions_home(id)?;
        let props = "<d:getcontentlength/><d:getlastmodified/>";
        let r =
            match self
                .dav(id)?
                .propfind(&format!("{home}versions/{}", file.number()), "1", props)
            {
                // A file nothing was changed in has no folder of versions.
                Err(Error::Http(404, _)) => return Ok(Vec::new()),
                r => r?,
            };
        let mut found: Vec<Version> = r
            .into_iter()
            .skip(1)
            .filter(|r| r.status == 200)
            .map(|r| Version {
                id: r
                    .href
                    .trim_end_matches('/')
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_string(),
                modified: r
                    .props
                    .find_text(DAV, "getlastmodified")
                    .and_then(|d| http_date(&d))
                    .unwrap_or(0),
                size: r
                    .props
                    .find_text(DAV, "getcontentlength")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            })
            .collect();
        found.sort_by_key(|v| std::cmp::Reverse(v.modified));
        Ok(found)
    }

    /// Make `version` the file's content again (the file as it is now becomes a version).
    pub fn restore_version(&self, id: i64, file: &File, version: &Version) -> Result<()> {
        let home = self.versions_home(id)?;
        let dav = self.dav(id)?;
        let to = dav.url(&format!("{home}restore/target"));
        let from = format!("{home}versions/{}/{}", file.number(), version.id);
        let r = dav.request("MOVE", &from, &[("Destination", &to)], "")?;
        match r.status {
            200..=299 => Ok(()),
            404 => Err(Error::NotFound(file.name.clone())),
            423 => Err(Error::Locked(file.name.clone())),
            s => Err(Error::Http(s, r.body)),
        }
    }

    /// Save `version` of `file` as `file` (its name) in folder `dest` of this device, under a
    /// name nothing there has; where it is.
    pub fn save_version(
        &self,
        id: i64,
        file: &File,
        version: &Version,
        dest: &Path,
        p: &Progress,
    ) -> Result<PathBuf> {
        let home = self.versions_home(id)?;
        let href = format!("{home}versions/{}/{}", file.number(), version.id);
        let dl = self
            .dav(id)?
            .get(&href, None)?
            .ok_or_else(|| Error::NotFound(file.name.clone()))?;
        p.set_total(version.size);
        let target = crate::transfer::free_path(dest, &file.name, false);
        crate::transfer::save(dl, &target, p)?;
        Ok(target)
    }
}
