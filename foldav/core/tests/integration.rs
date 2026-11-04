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
