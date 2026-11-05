//! Translations and the locale, through glibc: gettext with the `foldav` domain (catalogs in
//! `po/`, refreshed by `scripts/i18n.sh`). Mark text with `gettext`, `pgettext` where a word
//! needs its context; `ngettext` for counts; `gettext_noop` for text in a `const`; xgettext
//! finds these calls. Placeholders are named (`{title}`) and filled with `replace`.

use std::ffi::{CStr, CString, c_char, c_ulong};

const DOMAIN: &str = "foldav";

unsafe extern "C" {
    #[link_name = "gettext"]
    fn c_gettext(msgid: *const c_char) -> *mut c_char;
    #[link_name = "ngettext"]
    fn c_ngettext(one: *const c_char, many: *const c_char, n: c_ulong) -> *mut c_char;
    fn bindtextdomain(domain: *const c_char, dir: *const c_char) -> *mut c_char;
    fn bind_textdomain_codeset(domain: *const c_char, codeset: *const c_char) -> *mut c_char;
    fn textdomain(domain: *const c_char) -> *mut c_char;
}

fn c(s: &str) -> CString {
    CString::new(s).expect("no NUL in text")
}

/// The returned text is gettext's (or the argument itself): copy it before the next call.
fn owned(p: *const c_char) -> String {
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// Use the user's locale and this app's catalogs; before any text is shown or built (GtkBuilder
/// translates through the default domain set here).
pub fn init() {
    unsafe { libc::setlocale(libc::LC_ALL, c"".as_ptr()) };
    let dir = locale_dir();
    let (domain, dir) = (c(DOMAIN), c(&dir.to_string_lossy()));
    unsafe {
        bindtextdomain(domain.as_ptr(), dir.as_ptr());
        bind_textdomain_codeset(domain.as_ptr(), c"UTF-8".as_ptr());
        textdomain(domain.as_ptr());
    }
}

/// Installed (`make install-data`), the catalogs are in `../share/locale` next to the binary's
/// directory (/usr, ~/.local, /app); a build run uses the ones build.rs compiled.
fn locale_dir() -> std::path::PathBuf {
    let installed = std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.parent()?.join("share/locale")));
    let has_catalogs = |d: &std::path::Path| {
        std::fs::read_dir(d).is_ok_and(|mut langs| {
            langs.any(|l| l.is_ok_and(|l| l.path().join("LC_MESSAGES/foldav.mo").exists()))
        })
    };
    match installed {
        Some(d) if has_catalogs(&d) => d,
        _ => concat!(env!("OUT_DIR"), "/locale").into(),
    }
}

pub fn gettext(msgid: &str) -> String {
    let m = c(msgid);
    owned(unsafe { c_gettext(m.as_ptr()) })
}

/// The form of `one`/`many` for `n` in the user's language.
pub fn ngettext(one: &str, many: &str, n: u64) -> String {
    let (o, m) = (c(one), c(many));
    owned(unsafe { c_ngettext(o.as_ptr(), m.as_ptr(), n as c_ulong) })
}

/// `msgid` as meant in `context` (gettext's msgctxt), for words that translate differently.
pub fn pgettext(context: &str, msgid: &str) -> String {
    let key = format!("{context}\u{4}{msgid}");
    let t = gettext(&key);
    if t == key { msgid.to_string() } else { t }
}

/// Marks text in a constant for xgettext; `gettext` translates it where it is shown.
pub const fn gettext_noop(msgid: &str) -> &str {
    msgid
}
