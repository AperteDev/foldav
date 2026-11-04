# Agent notes

## Roadmap

- `docs/ROADMAP.md` holds the planned work only (features, open questions, what was ruled out);
  what is built is described by the code. Read it before working on a planned feature, and
  update it in the same change: remove what is done, add a plan or an open question, move a
  dropped plan under "Ruled out" with why.

## Todav and Notav

- Foldav is the sibling of Todav (`AperteDev/todav`) and Notav (`AperteDev/notav`), cloned next
  to this repository, and follows their conventions; when something here is not decided yet,
  look at how Notav does it (its `AGENTS.md`, `docs/ROADMAP.md` and code).
- The apps keep separate configuration and credentials: never read or write the others' files,
  keyring entries, settings or database.
- The code copied from Notav is listed in `docs/ROADMAP.md`, "Code from Todav and Notav"
  (the copies may diverge). A fix to one of them probably applies there too: port it or say so.

## Layout

- `foldav/core` (`foldav-core`): all server, storage and caching logic, no UI. `foldav/linux/app`
  (`foldav-linux`): the GTK app; static views are Blueprint files in `foldav/linux/app/ui/`,
  compiled by `build.rs`; dynamic UI stays in Rust code.
- Not built yet, planned as in Notav: the CLI, the Android app and bindings (`ffi/`), the
  packaging (Flatpak, RPM, AUR), CI and the Release workflow.

## Dependencies

- Standard library and platform SDKs first; a third-party crate only where it replaces real
  work, with the reason in its commit. Now: `ureq` (rustls, with the platform verifier),
  `rusqlite`, `quick-xml`, `gtk4`, `libadwaita`, `libsecret`, `libc`.
- `rusqlite` stays within 0.38 to 0.40 on the system SQLite, not bundled: distributions
  (Fedora) require that.

## Releases

- Add user-visible changes to `docs/CHANGELOG.md` under `## [Unreleased]` ([Keep a
  Changelog](https://keepachangelog.com/en/1.1.0/)), in the same commit as the change, and keep
  `docs/FEATURES.md` in step. Don't bump versions by hand (the Release workflow will, as in
  Notav).
- Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and
  `cargo test --workspace` before committing.

## Translations

- All text a user sees is translatable; one gettext domain, `foldav`, with catalogs in `po/`,
  refreshed by `scripts/i18n.sh`. The rules are Todav's: named placeholders, plurals with
  `ngettext` (to be added to `i18n.rs` with the first one), whole sentences, Polish translated
  for review, dates from the locale.

## Stored data

- An update carries on with what the previous version kept. The database (`foldav.sqlite` in
  the data folder) changes only by a new step at the end of `foldav/core/src/store.rs`
  `MIGRATIONS` that converts what is there; a released step is never edited, and each new step
  gets a test that opens a database of the step before. A database from a newer Foldav is
  refused (`store::open`), never written to. Synced settings (`.config/foldav/settings.json`)
  are also read by other devices: a synced key changed in meaning goes under a new name.

## Accounts

- Nextcloud accounts only; per-account `kv` keys are `a<id>_…` (`account::key`). Settings sync
  through the account picked in "Sync settings with" (device-only `settings_account`).

## Testing

- The app: `make -C foldav/linux sandbox` runs a separate copy (`FOLDAV_SANDBOX`, data in
  `target/sandbox`). To see and drive it, use the `waydriver` MCP server with
  `scripts/waydriver-app.sh` and `app_name` `foldav-waydriver` (build first); keyboard
  navigation works where OCR-based clicking does not (it needs a model download).
- Against the server: not set up yet (Notav's `integration` feature and gitignored `.env` are
  the model).
