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

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
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
    let (c, _dav, root) = fixture("upload");
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
    c.upload(A, &root, again, Taken::Replace, &Progress::default())
        .unwrap();
    assert_eq!(names(&c, &root).len(), 5);
    assert_eq!(read("a.txt"), b"second");

    // Cancelled: nothing is sent, and no half of a big file is left.
    let p = Progress::default();
    p.cancel();
    let r = c.upload(A, &root, &items[2..], Taken::KeepBoth, &p);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert_eq!(names(&c, &root).len(), 5);
    let _ = std::fs::remove_dir_all(&local);
}
