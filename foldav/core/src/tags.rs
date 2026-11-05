//! Tags on files (Nextcloud's `systemtags`): which a file has, giving it one and taking it away,
//! making a tag and deleting it. The tags of the server and their files are listed in
//! `places.rs`.

use crate::dav::OC;
use crate::places::Tag;
use crate::{Client, Error, File, Result};

impl Client {
    /// The DAV root, `/remote.php/dav`, of account `id`.
    fn dav_root(&self, id: i64) -> Result<String> {
        let home = self.files_home(id)?;
        Ok(home
            .split_once("/files/")
            .map_or("", |(r, _)| r)
            .to_string())
    }

    /// The tags `file` has.
    pub fn file_tags(&self, id: i64, file: &File) -> Result<Vec<Tag>> {
        let root = self.dav_root(id)?;
        let props = "<oc:id/><oc:display-name/>";
        Ok(self
            .dav(id)?
            .propfind(
                &format!("{root}/systemtags-relations/files/{}", file.number()),
                "1",
                props,
            )?
            .into_iter()
            .skip(1)
            .filter(|r| r.status == 200)
            .filter_map(|r| {
                Some(Tag {
                    id: r.props.find_text(OC, "id")?,
                    name: r.props.find_text(OC, "display-name")?,
                })
            })
            .collect())
    }

    /// Give `file` the tag `tag`, or take it away.
    pub fn set_file_tag(&self, id: i64, file: &File, tag: &str, on: bool) -> Result<()> {
        let root = self.dav_root(id)?;
        let href = format!("{root}/systemtags-relations/files/{}/{tag}", file.number());
        let r = self
            .dav(id)?
            .request(if on { "PUT" } else { "DELETE" }, &href, &[], "")?;
        match r.status {
            // Has it already, or has not.
            200..=299 | 409 | 404 => Ok(()),
            s => Err(Error::Http(s, r.body)),
        }
    }

    /// Make a tag; the one of that name if there is one already.
    pub fn create_tag(&self, id: i64, name: &str) -> Result<Tag> {
        let root = self.dav_root(id)?;
        let body = format!(
            r#"{{"name":{},"userVisible":true,"userAssignable":true,"canAssign":true}}"#,
            crate::json::Json::Str(name.into())
        );
        let r = self.dav(id)?.request(
            "POST",
            &format!("{root}/systemtags"),
            &[("Content-Type", "application/json")],
            &body,
        )?;
        match r.status {
            // The server gives its id in a header; asking for the tags is as short.
            200..=299 | 409 => self
                .tags(id)?
                .into_iter()
                .find(|t| t.name == name)
                .ok_or_else(|| Error::Exists(name.into())),
            s => Err(Error::Http(s, r.body)),
        }
    }

    /// Delete the tag `tag` and take it off all the files.
    pub fn delete_tag(&self, id: i64, tag: &str) -> Result<()> {
        let root = self.dav_root(id)?;
        self.dav(id)?
            .delete(&format!("{root}/systemtags/{tag}"), None)
    }
}
