//! Runs against the real server from the workspace `.env` (`FOLDAV_URL`, `FOLDAV_USER`,
//! `FOLDAV_PASS`): `cargo test -p foldav-core --features integration`. Each test works in its
//! own `it-foldav/<name>/` folder, deleted first; settings sync is off.
#![cfg(feature = "integration")]

use foldav_core::dav::Dav;
use foldav_core::{Client, Error};

fn get(k: &str) -> String {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../.env"))
        .unwrap_or_default();
    std::env::var(k)
        .ok()
        .or_else(|| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_string))
        })
        .unwrap_or_else(|| panic!("{k} not set"))
}

const A: i64 = 1;

/// The client with the Test account, "another client" straight to the server, and the test's
/// folder (relative to the Files root).
fn fixture(test: &str) -> (Client, Dav, String) {
    let (url, user, pass) = (get("FOLDAV_URL"), get("FOLDAV_USER"), get("FOLDAV_PASS"));
    let dir = std::env::temp_dir().join(format!("foldav-it-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let c = Client::open(dir.to_string_lossy().into()).unwrap();
    let id = c
        .add_account(url.clone(), user.clone(), String::new())
        .unwrap();
    assert_eq!(id, A);
    c.connect(A, pass.clone()).unwrap();
    let dav = Dav::new(&url, &user, &pass);
    let home = c.files_home(A).unwrap();
    let root = format!("it-foldav/{test}");
    let _ = dav.delete(&format!("{home}{root}"), None);
    dav.mkcol_all(&home, &format!("{home}{root}/")).unwrap();
    (c, dav, root)
}

fn names(c: &Client, path: &str) -> Vec<String> {
    c.list(A, path)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect()
}

#[test]
fn folders_are_made_renamed_moved_copied_and_deleted_on_the_server() {
    let (c, dav, root) = fixture("ops");
    let home = c.files_home(A).unwrap();
    let at = |p: &str| format!("{root}/{p}");

    c.new_folder(A, &root, "a").unwrap();
    assert!(matches!(c.new_folder(A, &root, "a"), Err(Error::Exists(_))));
    assert!(matches!(
        c.new_folder(A, &root, "x/y"),
        Err(Error::Invalid(_))
    ));
    let r = dav
        .request("PUT", &format!("{home}{}", at("a/f.txt")), &[], "hello")
        .unwrap();
    assert!((200..300).contains(&r.status), "PUT {}", r.status);

    // A copy of a folder with what it holds, under a free name.
    c.copy_to(A, &[at("a")], &root).unwrap();
    assert_eq!(names(&c, &root), ["a", "a (copy)"]);
    assert_eq!(names(&c, &at("a (copy)")), ["f.txt"]);

    // Rename, and its undo.
    let step = c.rename(A, &at("a"), "b").unwrap();
    assert_eq!(names(&c, &root), ["a (copy)", "b"]);
    assert!(matches!(
        c.rename(A, &at("b"), "a (copy)"),
        Err(Error::Exists(_))
    ));
    c.undo(step).unwrap();
    assert_eq!(names(&c, &root), ["a", "a (copy)"]);

    // Move into a folder, and not into itself.
    let step = c.move_to(A, &[at("a (copy)")], &at("a")).unwrap();
    assert_eq!(names(&c, &at("a")), ["a (copy)", "f.txt"]);
    assert!(matches!(
        c.move_to(A, &[at("a")], &at("a/a (copy)")),
        Err(Error::Invalid(_))
    ));
    c.undo(step).unwrap();
    assert_eq!(names(&c, &at("a")), ["f.txt"]);

    // Deleted (to the trash), then back from it.
    let step = c.delete(A, &[at("a (copy)"), at("a")]).unwrap();
    assert!(names(&c, &root).is_empty());
    c.undo(step).unwrap();
    assert_eq!(names(&c, &root), ["a", "a (copy)"]);
    assert_eq!(names(&c, &at("a")), ["f.txt"]);
    assert!(
        matches!(c.undo(None), Err(Error::NotFound(_))),
        "nothing left"
    );
}

#[test]
fn listings_are_kept_and_follow_the_folders_etag() {
    let (c, dav, root) = fixture("cache");
    let home = c.files_home(A).unwrap();
    let file = format!("{home}{root}/f.txt");
    assert_eq!(c.cached(A, &root), None, "not listed yet");

    assert!(c.list(A, &root).unwrap().is_empty());
    assert_eq!(
        c.cached(A, &root),
        Some(vec![]),
        "kept, an empty folder too"
    );

    // A change on the server changes the folder's ETag: the next listing has it.
    let r = dav.request("PUT", &file, &[], "hello").unwrap();
    assert!((200..300).contains(&r.status), "PUT {}", r.status);
    assert_eq!(c.cached(A, &root), Some(vec![]), "until it is asked again");
    assert_eq!(names(&c, &root), ["f.txt"]);
    assert_eq!(c.cached(A, &root).unwrap().len(), 1);

    // Unchanged: the same, from the cache.
    assert_eq!(names(&c, &root), ["f.txt"]);
    dav.delete(&file, None).unwrap();
    assert!(names(&c, &root).is_empty());

    // A folder gone from the server is no listing any more.
    assert!(matches!(
        c.list(A, &format!("{root}/nope")),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn favorites_go_up_and_come_down_and_changes_elsewhere_are_seen() {
    let (c, dav, root) = fixture("favorites");
    let home = c.files_home(A).unwrap();
    c.new_folder(A, &root, "one").unwrap();
    c.new_folder(A, &root, "two").unwrap();
    let (one, two) = (format!("{root}/one"), format!("{root}/two"));
    let on_server = |p: &str| {
        dav.favorites(&home)
            .unwrap()
            .iter()
            .any(|(href, _)| href.trim_end_matches('/') == format!("{home}{p}"))
    };
    let kept = |p: &str| c.favorites(A).iter().any(|f| f.path == p && f.folder);
    c.sync_favorites(A).unwrap(); // what the account had already

    // Starred here: kept at once, on the server as soon as it can be reached.
    c.set_favorite(A, &one, true, true).unwrap();
    assert!(kept(&one) && on_server(&one));
    assert!(!c.sync_favorites(A).unwrap(), "nothing changed meanwhile");

    // Starred on another device: seen by the next sync.
    dav.set_favorite(&format!("{home}{two}"), true).unwrap();
    assert!(!kept(&two));
    assert!(c.sync_favorites(A).unwrap());
    assert!(kept(&two));

    // Unstarred here and there.
    c.set_favorite(A, &one, true, false).unwrap();
    assert!(!kept(&one) && !on_server(&one));
    dav.set_favorite(&format!("{home}{two}"), false).unwrap();
    assert!(c.sync_favorites(A).unwrap());
    assert!(!kept(&two));
}

#[test]
fn files_are_opened_from_a_kept_copy_and_downloaded_with_their_folders() {
    use foldav_core::Progress;
    let (c, dav, root) = fixture("transfer");
    let home = c.files_home(A).unwrap();
    let put = |p: &str, body: &str| {
        let r = dav
            .request("PUT", &format!("{home}{root}/{p}"), &[], body)
            .unwrap();
        assert!((200..300).contains(&r.status), "PUT {}", r.status);
    };
    c.new_folder(A, &root, "dir").unwrap();
    put("a.txt", "hello");
    put("dir/b.txt", "inside");
    let file = |name: &str| {
        c.list(A, &root)
            .unwrap()
            .into_iter()
            .find(|f| f.name == name)
            .unwrap()
    };

    // Fetched, then the same copy while the server's file is the same.
    let p = Progress::default();
    let copy = c.open_file(A, &file("a.txt"), &p).unwrap();
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "hello");
    assert_eq!((p.done(), p.total()), (5, 5));
    let p = Progress::default();
    assert_eq!(c.open_file(A, &file("a.txt"), &p).unwrap(), copy);
    assert_eq!(p.done(), 0, "nothing fetched again");

    // Changed on the server: the copy is made again.
    // The server keeps the ETag of a file rewritten within a moment of its last change.
    std::thread::sleep(std::time::Duration::from_secs(3));
    put("a.txt", "hello again");
    let copy = c
        .open_file(A, &file("a.txt"), &Progress::default())
        .unwrap();
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "hello again");

    // Downloaded: a folder with what is in it; a second time under another name.
    let dest = std::env::temp_dir().join(format!("foldav-it-download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).unwrap();
    let both = [file("a.txt"), file("dir")];
    c.download(A, &both, &dest, &Progress::default()).unwrap();
    c.download(A, &both[1..], &dest, &Progress::default())
        .unwrap();
    let read = |p: &str| std::fs::read_to_string(dest.join(p)).unwrap();
    assert_eq!(read("a.txt"), "hello again");
    assert_eq!(read("dir/b.txt"), "inside");
    assert_eq!(read("dir (copy)/b.txt"), "inside");

    // Cancelled: nothing is left.
    let p = Progress::default();
    p.cancel();
    let r = c.download(A, &both[1..], &dest, &p);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert!(!dest.join("dir (copy 2)").exists());
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn files_and_folders_are_uploaded_whole_or_in_chunks() {
    use foldav_core::{Progress, Taken};
    let (c, dav, root) = fixture("upload");
    let local = std::env::temp_dir().join(format!("foldav-it-upload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    std::fs::create_dir_all(local.join("tree/sub")).unwrap();
    std::fs::write(local.join("a.txt"), "hello").unwrap();
    std::fs::write(local.join("tree/b.txt"), "b").unwrap();
    std::fs::write(local.join("tree/sub/c ż.txt"), "c").unwrap();
    // Over the size where a file goes in chunks (the last one a short one).
    let big: Vec<u8> = (0..(40u32 << 20) + 5).map(|i| (i % 251) as u8).collect();
    std::fs::write(local.join("big.bin"), &big).unwrap();

    let items = [
        local.join("a.txt"),
        local.join("tree"),
        local.join("big.bin"),
    ];
    let p = Progress::default();
    c.upload(A, &root, &items, Taken::KeepBoth, &p).unwrap();
    assert_eq!(
        (p.done(), p.total()),
        (big.len() as u64 + 7, big.len() as u64 + 7)
    );
    assert_eq!(names(&c, &root), ["tree", "a.txt", "big.bin"]);
    assert_eq!(names(&c, &format!("{root}/tree/sub")), ["c ż.txt"]);
    let read = |path: &str| {
        let f = c
            .list(A, &root)
            .unwrap()
            .into_iter()
            .find(|f| f.name == path)
            .unwrap();
        let copy = c.open_file(A, &f, &Progress::default()).unwrap();
        std::fs::read(copy).unwrap()
    };
    assert_eq!(read("a.txt"), b"hello");
    assert!(read("big.bin") == big, "the chunks came together");

    // Names taken: asked about, kept both, replaced, or left out.
    let all = ["a.txt".to_string(), "new".to_string(), "tree".to_string()];
    assert_eq!(c.taken(A, &root, &all).unwrap(), ["a.txt", "tree"]);
    std::fs::write(local.join("a.txt"), "second").unwrap();
    let again = &items[..2];
    c.upload(A, &root, again, Taken::KeepBoth, &Progress::default())
        .unwrap();
    assert_eq!(
        names(&c, &root),
        ["tree", "tree (copy)", "a (copy).txt", "a.txt", "big.bin"]
    );
    c.upload(A, &root, again, Taken::Skip, &Progress::default())
        .unwrap();
    assert_eq!(names(&c, &root).len(), 5);
    // The server keeps the ETag of a file rewritten within a moment of its last change.
    std::thread::sleep(std::time::Duration::from_secs(3));
    std::fs::write(local.join("tree/b.txt"), "b changed").unwrap();
    c.upload(A, &root, again, Taken::Replace, &Progress::default())
        .unwrap();
    assert_eq!(names(&c, &root).len(), 5);
    assert_eq!(read("a.txt"), b"second");
    // The small files of the tree went in one bulk request, replacing what was there.
    let home = c.files_home(A).unwrap();
    let on_server = |p: &str| {
        dav.request("GET", &format!("{home}{root}/{p}"), &[], "")
            .unwrap()
            .body
    };
    assert_eq!(on_server("tree/b.txt"), "b changed");
    assert_eq!(on_server("tree/sub/c ż.txt"), "c");

    // Many small files, in several bulk requests (a hundred to a request).
    std::fs::create_dir_all(local.join("many")).unwrap();
    for i in 0..230 {
        std::fs::write(local.join(format!("many/f{i:03}.txt")), format!("file {i}")).unwrap();
    }
    let p = Progress::default();
    c.upload(A, &root, &[local.join("many")], Taken::KeepBoth, &p)
        .unwrap();
    assert_eq!(names(&c, &format!("{root}/many")).len(), 230);
    assert_eq!((p.done(), p.total()), (p.total(), p.total()));
    assert_eq!(on_server("many/f229.txt"), "file 229");

    // Cancelled: nothing is sent, and no half of a big file is left.
    let p = Progress::default();
    p.cancel();
    let r = c.upload(A, &root, &items[2..], Taken::KeepBoth, &p);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert_eq!(names(&c, &root).len(), 6);
    let _ = std::fs::remove_dir_all(&local);
}

#[test]
fn a_change_saved_to_an_opened_copy_goes_back_unless_the_server_changed() {
    use foldav_core::{Progress, Saved};
    let (c, dav, root) = fixture("edits");
    let home = c.files_home(A).unwrap();
    let url = format!("{home}{root}/a.txt");
    let put = |body: &str| {
        let r = dav.request("PUT", &url, &[], body).unwrap();
        assert!((200..300).contains(&r.status), "PUT {}", r.status);
    };
    let server = || dav.request("GET", &url, &[], "").unwrap().body;
    let file = || {
        c.list(A, &root)
            .unwrap()
            .into_iter()
            .find(|f| f.name == "a.txt")
            .unwrap()
    };
    // The server keeps the ETag of a file rewritten within a moment of its last change; a copy
    // is left to settle before it is sent.
    let wait = || std::thread::sleep(std::time::Duration::from_secs(3));
    put("v1");
    wait();
    let copy = c.open_file(A, &file(), &Progress::default()).unwrap();
    assert!(!c.has_changes());

    std::fs::write(&copy, "v2 edited").unwrap();
    wait();
    assert!(c.has_changes());
    let saved = c.save_changes(&Progress::default());
    assert!(
        matches!(saved.as_slice(), [(n, Ok(Saved::Uploaded))] if n == "a.txt"),
        "{saved:?}"
    );
    assert_eq!(server(), "v2 edited");
    assert!(!c.has_changes());

    // The server's file changed meanwhile: it stays, the edited copy is kept beside it.
    std::fs::write(&copy, "v3 local").unwrap();
    put("v3 server");
    wait();
    let saved = c.save_changes(&Progress::default());
    assert!(
        matches!(saved.as_slice(), [(_, Ok(Saved::Conflict(n)))] if n == "a (conflicted copy).txt"),
        "{saved:?}"
    );
    assert_eq!(server(), "v3 server");
    assert_eq!(names(&c, &root), ["a (conflicted copy).txt", "a.txt"]);
    // Opened again: the current file, not the old copy.
    let copy = c.open_file(A, &file(), &Progress::default()).unwrap();
    assert_eq!(std::fs::read_to_string(copy).unwrap(), "v3 server");
}

#[test]
fn places_list_recent_favorites_searches_and_the_trash_bin() {
    use foldav_core::places::{self, FAVORITES, RECENT, TRASH};
    let (c, dav, root) = fixture("places");
    let home = c.files_home(A).unwrap();
    let put = |p: &str| {
        let r = dav
            .request("PUT", &format!("{home}{root}/{p}"), &[], "x")
            .unwrap();
        assert!((200..300).contains(&r.status), "PUT {}", r.status);
    };
    c.new_folder(A, &root, "sub").unwrap();
    put("alpha-report.txt");
    put("sub/beta.txt");
    let here = |p: &str| format!("{root}/{p}");
    let in_place = |place: &str| -> Vec<String> {
        c.list(A, place)
            .unwrap()
            .into_iter()
            .map(|f| f.path)
            .collect()
    };

    // The latest changed files, folders left out.
    let recent = in_place(RECENT);
    assert!(recent.contains(&here("alpha-report.txt")), "{recent:?}");
    assert!(!recent.contains(&here("sub")));

    dav.set_favorite(&format!("{home}{}", here("sub/beta.txt")), true)
        .unwrap();
    assert!(in_place(FAVORITES).contains(&here("sub/beta.txt")));

    // By name, in a folder and below.
    assert_eq!(
        in_place(&places::search_place(&root, "ALPHA")),
        [here("alpha-report.txt")]
    );
    assert_eq!(
        in_place(&places::search_place(&here("sub"), "alpha")).len(),
        0
    );

    // Deleted: in the bin, restored, deleted again and removed from it for good.
    c.delete(A, &[here("alpha-report.txt")]).unwrap();
    let bin = c.list(A, TRASH).unwrap();
    let gone = bin
        .iter()
        .find(|f| f.name == "alpha-report.txt")
        .expect("in the bin");
    c.trash_restore(A, std::slice::from_ref(&gone.path))
        .unwrap();
    assert!(names(&c, &root).contains(&"alpha-report.txt".to_string()));
    c.delete(A, &[here("alpha-report.txt")]).unwrap();
    let bin = c.list(A, TRASH).unwrap();
    let gone = bin.iter().find(|f| f.name == "alpha-report.txt").unwrap();
    c.trash_delete(A, std::slice::from_ref(&gone.path)).unwrap();
    assert!(
        !c.list(A, TRASH)
            .unwrap()
            .iter()
            .any(|f| f.name == "alpha-report.txt")
    );

    // Tags the server has, and the shares of the user (nothing shared by this test).
    c.tags(A).unwrap();
    c.list(A, places::SHARED_WITH_YOU).unwrap();
    c.list(A, places::SHARED_WITH_OTHERS).unwrap();
    c.list(A, places::SHARED_BY_LINK).unwrap();
}

#[test]
fn the_server_says_what_it_offers_and_what_is_used() {
    let (c, _dav, _root) = fixture("server");
    let f = c.features(A).unwrap();
    assert!(f.trash && f.sharing, "{f:?}");
    let q = c.quota(A).unwrap();
    assert!(q.used > 0, "{q:?}");
}

#[test]
fn previews_are_fetched_once_and_kept_by_etag() {
    let (c, dav, root) = fixture("thumbs");
    let home = c.files_home(A).unwrap();
    // A 1×1 PNG.
    let png: Vec<u8> = vec![
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1f, 0x15, 0xc4, 0x89, 0, 0, 0, 0x0d, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xf0, 0x1f, 0, 5, 0, 1, 0xff, 0x89, 0x99,
        0x3d, 0x1d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    let r = dav
        .upload(
            "PUT",
            &format!("{home}{root}/dot.png"),
            &[],
            png.len() as u64,
            || Ok(Box::new(std::io::Cursor::new(png.clone()))),
        )
        .unwrap();
    assert!((200..300).contains(&r.status), "PUT {}", r.status);
    dav.request("PUT", &format!("{home}{root}/plain.bin"), &[], "x")
        .unwrap();
    let files = c.list(A, &root).unwrap();
    let file = |n: &str| files.iter().find(|f| f.name == n).unwrap();
    let path = c
        .thumbnail(A, file("dot.png"), 64)
        .unwrap()
        .expect("a preview");
    assert!(std::fs::metadata(&path).unwrap().len() > 0);
    assert_eq!(c.thumbnail_path(A, file("dot.png"), 64), Some(path.clone()));
    // Kept: the same path again, nothing fetched.
    assert_eq!(c.thumbnail(A, file("dot.png"), 64).unwrap(), Some(path));
    // A file the server has no preview for.
    assert_eq!(c.thumbnail(A, file("plain.bin"), 64).unwrap(), None);
}

#[test]
fn files_are_shared_by_link_tagged_and_keep_versions() {
    use foldav_core::{ShareKind, places};
    let (c, dav, root) = fixture("nextcloud");
    let home = c.files_home(A).unwrap();
    let url = format!("{home}{root}/a.txt");
    let put = |body: &str| {
        let r = dav.request("PUT", &url, &[], body).unwrap();
        assert!((200..300).contains(&r.status), "PUT {}", r.status);
    };
    put("one");
    let path = format!("{root}/a.txt");
    let file = || {
        c.list(A, &root)
            .unwrap()
            .into_iter()
            .find(|f| f.name == "a.txt")
            .unwrap()
    };

    // A public link, changed and taken down.
    assert!(c.file_shares(A, &path).unwrap().is_empty());
    let link = c
        .share_link(A, &path, false, Some("s3cret-Passw0rd!"), None)
        .unwrap();
    assert_eq!(link.kind, ShareKind::Link);
    assert!(
        link.url.as_deref().is_some_and(|u| u.starts_with("http")),
        "{link:?}"
    );
    assert!(link.has_password);
    let listed = c.file_shares(A, &path).unwrap();
    assert_eq!(listed.len(), 1);
    let changed = c
        .update_share(A, &link.id, None, Some(""), Some("2099-01-01"))
        .unwrap();
    assert!(!changed.has_password);
    assert_eq!(changed.expires.as_deref(), Some("2099-01-01"));
    c.delete_share(A, &link.id).unwrap();
    assert!(c.file_shares(A, &path).unwrap().is_empty());
    assert!(c.find_sharees(A, "zzzz-nobody").unwrap().is_empty());

    // A tag: made, put on the file, listed with its files, taken off.
    let tag = c.create_tag(A, "foldav-it-tag-2").unwrap();
    assert_eq!(c.create_tag(A, "foldav-it-tag-2").unwrap(), tag);
    c.set_file_tag(A, &file(), &tag.id, true).unwrap();
    assert_eq!(c.file_tags(A, &file()).unwrap(), std::slice::from_ref(&tag));
    let tagged: Vec<_> = c.list(A, &places::tag_place(&tag.id)).unwrap();
    assert_eq!(
        tagged.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        [path.as_str()]
    );
    c.set_file_tag(A, &file(), &tag.id, false).unwrap();
    assert!(c.file_tags(A, &file()).unwrap().is_empty());
    // Only an administrator may delete a tag: the Test user gets "forbidden", and the tag stays.
    let deleted = c.delete_tag(A, &tag.id);
    assert!(
        matches!(deleted, Ok(()) | Err(Error::Http(403, _))),
        "{deleted:?}"
    );

    // Versions: the file as it was before it was rewritten, put back.
    std::thread::sleep(std::time::Duration::from_secs(2));
    put("two");
    let f = file();
    let versions = c.versions(A, &f).unwrap();
    assert!(!versions.is_empty(), "{versions:?}");
    assert_eq!(versions[0].size, 3);
    let oldest = versions.last().unwrap();
    c.restore_version(A, &f, oldest).unwrap();
    assert_eq!(dav.request("GET", &url, &[], "").unwrap().body, "one");
}

#[test]
fn folders_come_as_one_zip_and_files_are_copied_through_this_device() {
    use foldav_core::{Progress, Taken};
    let (c, dav, root) = fixture("zip");
    let home = c.files_home(A).unwrap();
    c.new_folder(A, &root, "dir").unwrap();
    for (p, body) in [("a.txt", "hello"), ("dir/b.txt", "inside")] {
        let r = dav
            .request("PUT", &format!("{home}{root}/{p}"), &[], body)
            .unwrap();
        assert!((200..300).contains(&r.status), "PUT {}", r.status);
    }
    let files = c.list(A, &root).unwrap();
    let dest = std::env::temp_dir().join(format!("foldav-it-zip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).unwrap();

    // One folder: named after it; several: after the folder they are in. A ZIP starts "PK".
    let one = c
        .download_zip(A, &root, &files[..1], &dest, &Progress::default())
        .unwrap();
    assert_eq!(one.file_name().unwrap(), "dir.zip");
    let all = c
        .download_zip(A, &root, &files, &dest, &Progress::default())
        .unwrap();
    assert_eq!(all.file_name().unwrap(), "zip.zip");
    for z in [one, all] {
        assert_eq!(&std::fs::read(z).unwrap()[..2], b"PK");
    }

    // Copied to another folder of the account the way it goes to another account.
    c.new_folder(A, &root, "to").unwrap();
    let to = format!("{root}/to");
    c.copy_across(A, &files, A, &to, Taken::KeepBoth, &Progress::default())
        .unwrap();
    assert_eq!(names(&c, &to), ["dir", "a.txt"]);
    assert_eq!(names(&c, &format!("{to}/dir")), ["b.txt"]);
    let _ = std::fs::remove_dir_all(&dest);
}

#[test]
fn a_name_taken_in_a_move_or_copy_is_replaced_kept_both_or_skipped() {
    use foldav_core::Taken;
    let (c, dav, root) = fixture("paste");
    let home = c.files_home(A).unwrap();
    let at = |p: &str| format!("{root}/{p}");
    let put = |p: &str, body: &str| {
        let r = dav
            .request("PUT", &format!("{home}{}", at(p)), &[], body)
            .unwrap();
        assert!((200..300).contains(&r.status), "PUT {}", r.status);
    };
    let body = |p: &str| {
        dav.request("GET", &format!("{home}{}", at(p)), &[], "")
            .unwrap()
            .body
    };
    c.new_folder(A, &root, "from").unwrap();
    c.new_folder(A, &root, "to").unwrap();
    put("from/a.txt", "new");
    put("to/a.txt", "old");
    let src = [at("from/a.txt")];

    // Skipped: nothing changes. Kept both: a copy name. Replaced: the content of the new.
    c.paste_into(A, &src, &at("to"), false, Taken::Skip)
        .unwrap();
    assert_eq!(
        (names(&c, &at("to")), body("to/a.txt")),
        (["a.txt"].map(String::from).to_vec(), "old".into())
    );
    c.paste_into(A, &src, &at("to"), false, Taken::KeepBoth)
        .unwrap();
    assert_eq!(names(&c, &at("to")), ["a (copy).txt", "a.txt"]);
    c.paste_into(A, &src, &at("to"), false, Taken::Replace)
        .unwrap();
    assert_eq!(body("to/a.txt"), "new");

    // A move replaces too, and the source is gone; a copy into the folder it is in is named.
    put("from/a.txt", "newer");
    c.paste_into(A, &src, &at("to"), true, Taken::Replace)
        .unwrap();
    assert_eq!(body("to/a.txt"), "newer");
    assert_eq!(names(&c, &at("from")), Vec::<String>::new());
    c.paste_into(A, &[at("to/a.txt")], &at("to"), false, Taken::Replace)
        .unwrap();
    assert_eq!(
        names(&c, &at("to")),
        ["a (copy 2).txt", "a (copy).txt", "a.txt"]
    );
}
