# Foldav — Roadmap

Planned work, not yet done; the order is not fixed. Built so far: the scaffold (accounts, settings, a folder listing); the rest is the plan.

## Overview

Foldav (*folder* + DAV) is a file manager for GNOME, made for the files in your Nextcloud. It
is the sibling of [Todav](https://github.com/AperteDev/todav) (tasks and calendar over CalDAV)
and [Notav](https://github.com/AperteDev/notav) (Markdown notes over WebDAV) and follows their
design: one Rust core with all server, storage and caching logic, thin front-ends, the same
Nextcloud and GNOME Online Accounts integration.

**Why**: Nautilus reaches Nextcloud through GVfs's generic WebDAV backend, which knows nothing
of Nextcloud:

- Slow folder operations: copying or deleting a folder of many small files goes file by file
  from the client and takes minutes, where Nextcloud's web Files does it on the server in
  seconds (one `COPY`, `MOVE` or `DELETE` of the whole folder).
- No thumbnails: images, videos and documents show generic icons, where the web app shows
  previews the server has made already.
- No Nextcloud features: the trash bin (a deletion through Nautilus is gone for good), file
  versions, sharing (links and with users or groups), favorites, tags, "Shared with me".

**Goals**

- Every operation the server can do in one request is done in one request: copies, moves and
  deletions of whole folders on the server, many small uploads in one bulk request.
- Fast to browse: listings and thumbnails cached locally and shown at once, refreshed in the
  background (folder ETags tell what changed, as in Notav).
- What Nextcloud's web Files offers, natively: trash, versions, sharing, favorites, recent
  files, search, tags.
- Works with the rest of the desktop: files dragged to and from Nautilus, opened in their
  default apps, the account from GNOME Online Accounts.
- Minimal dependencies: standard library and platform SDKs first; a third-party crate only
  where it replaces real work.

## Decisions

- Name Foldav, app ID `dev.aperte.Foldav`, gettext domain `foldav`, its own repository
  (`AperteDev/foldav`) (decided 2026-11-01).
- **Separate from Todav and Notav in configuration and credentials**: its own sign-in, keyring
  entries under its own app ID, its own synced settings file (`.config/foldav/settings.json`)
  and its own database. Talks to the others only through their D-Bus APIs and links.
- **Not a sync client**: files stay on the server; the Nextcloud desktop client keeps synced
  copies of folders, Foldav browses and manages all of them. Only what makes browsing fast is
  kept locally (listings, thumbnails), and the files opened in other apps (see
  [Open questions](#open-questions-and-risks)).

## Code from Todav and Notav

Copied, not shared, as Notav did from Todav; the copies may diverge.

| Piece | From | Notes |
| --- | --- | --- |
| Login Flow v2, app password sign-in, minimal JSON | Todav, Notav (`login.rs`, `json.rs`) | As is |
| HTTPS trusting the system's certificate authorities, so a self-hosted server with its own CA connects (ureq's `platform-verifier`) | Notav `core/src/net.rs` | Every agent built with `tls_config(net::tls())`; without it ureq trusts only its built-in list and fails with `UnknownIssuer` (AperteDev/todav#3) |
| HTTP errors showing an error page's text, not its HTML | Notav `core/src/lib.rs` (`Error`'s `Display`, `html_text`) | As is |
| WebDAV XML, multistatus, percent-encoding, requests with retries | Notav `core/src/dav.rs` | Plus `COPY`, `MOVE`, `SEARCH`, chunked and bulk uploads |
| Accounts, synced settings, `Error`, `Client::open`, store migrations, `kv` | Notav `core/src/account.rs`, `settings.rs`, `lib.rs`, `store.rs` | Nextcloud accounts only (no local one) |
| Favorites, trash bin, versions, server features | Notav `core/src/files.rs` | For any file, not only notes |
| GNOME Online Accounts, gettext, Secret Service, adding accounts, Preferences, About, update banner | Notav `linux/app/src/` | As is, adapted |
| Build, packaging, release | Notav (`build.rs`, `linux/Makefile`, Flatpak, `scripts/`, `.github/workflows/`, RPM, COPR) | No GtkSourceView |

Once a third app copies the same code, a shared crate in its own repository becomes worth it
(see Notav's roadmap, "Later").

## Planned features

1. **Browsing**, what is left of it (the Nautilus-like window with icons, list and columns,
   path bar, history, favorite folders, folder tree, accounts, thumbnails and search by name
   is done): a limit to the listing and thumbnail caches' size (now they go by age), and a
   long-offline mode that shows them without asking; a zoom for the icons; sorting also in
   the icons and columns; the last folder and account restored at start.
2. **The web app's places in the sidebar**, what is left of them (All files is the tree's
   "Home"; Recent, Starred, Shared with you, Shared with others, Shared by link, Tags and
   Deleted files are built, each shown where the server's capabilities have it, and the
   storage used): *Personal files*: only the user's own, without what is shared with them and
   external storage; *Pending shares* (`shares/pending`, to accept or decline), *File
   requests* (upload-only links, Nextcloud 32), *Deleted shares* (`deletedshares`, to
   restore); *Files settings* (Preferences: sorting, hidden files, the web app's own settings
   in the browser). The places' files come in the server's order, whatever the view sorts by.
3. **Operations on the server**, what is left of them (new folder, rename, copy, move, delete,
   Ctrl+Z, dragging files inside the window and the choice between replacing, keeping both
   and skipping when a name is taken are done): a
   deletion asks nothing and offers Undo, but undoing needs the trash bin, so check the
   server's capabilities (`Features`, as in Notav) and ask first where it has none (`Features`
   is there now); undoing a move that replaced a file does not bring the replaced file back;
   moving (not only copying) between accounts, and pasting between them.
4. **Transfers**, what is left of them (downloads, a folder or several files as one ZIP,
   uploads in one request, in chunks or many small files in one bulk request, drag and drop
   inside the window and to and from other apps, copies between accounts, and the sidebar's
   rows for running actions and their cancel, are done): extracting a ZIP downloaded as
   folders (a folder dragged out of the window is still fetched file by file). Transfers queue,
   resume after a dropped connection (a chunk is sent again, a download starts over), and
   finish before the app quits (now closing the window stops them). A stalled connection is
   not given up on until the system does (`ureq` has no idle timeout). The chunks of a big file
   go three at a time, where the server takes five (`max_parallel_count`; not read from the
   capabilities). Dragging to and from
   other apps is not tested against Files itself: drags between the window's own views are
   (by hand), and what a drag out hands over was read back through the clipboard (a
   `text/uri-list` of the downloaded copies); a drop from another app is the same code as an
   upload and is not driven.
5. **Opening files**, what is left of it (a file opens in its default app from a copy kept in
   the cache folder, checked against the server's ETag, deleted after a month unused; a
   change saved to the copy is uploaded back, with the ETag checked first and a conflict
   kept beside the file; Open With…, and several files opened at once): a limit to the
   copies' total size; a copy of a file renamed or moved on the server meanwhile is sent to its old
   path (as a conflicted copy, the ETag check fails there).
6. **Nextcloud's features** on files, what is left of them (the places that list them are
   above; the trash bin, versions, sharing by link and with users and groups, tags on a file,
   and search by name are built):
   - Versions: a label for a version, deleting one.
   - Sharing: shares with federated users and by email, a share's note and label, the
     permissions of a folder share beyond editing, accepting pending shares, the share
     sheet for several files at once; sharing is untested with a second user (the Test
     account is alone on its server), only by link.
   - Tags: removing a tag (the server lets only an administrator), tags in the columns' preview.
   - Search by content (the unified search).
   - "Open in the browser" for what Foldav does not do (comments, activity, apps' own views).
7. **Change notifications**, what is left (the folder shown is polled by its ETag every 30
   seconds and when the window comes to the front): Nextcloud's Client Push (`notify_push`,
   which this server has) for an instant refresh, and for the places and the sidebar's tree.

Later (v2):

- Image and PDF viewer in the app (with the previews already cached); media streamed rather
  than downloaded first.
- Links from Notav and Todav to files (`foldav://`), and Notav's attachments in Foldav.
- Offline folders: chosen folders kept on the device and synced, for those without the
  desktop client.

## Open questions and risks

- **Opening files in other apps**: a local copy in a cache folder is built (kept a month,
  cleaned at start) and looked at every 5 seconds for saves, which are sent back once the
  file has rested 2 seconds; an app that touches the copy without a change sends it again.
  A FUSE mount or a GVfs
  backend would let apps open the server's paths directly but is much more work.
  The server may keep a file's ETag when it is rewritten within a second of its last change,
  so a copy can then be taken for current (the integration test waits for it).
- **Bulk operations**: bulk upload (`/remote.php/dav/bulk`, Nextcloud 28 and up: files up to
  256 KB, up to 100 or 4 MB a request, each with its MD5) falls back to one request a file when
  the server does not answer that every file was stored; which versions have ZIP downloads of
  folders over WebDAV (this one, 35, has) and what Foldav does where they do not; whether a
  server-side `COPY` of a huge folder can time out on the client while the server is still
  working.
- **Thumbnails**: the preview endpoint is not WebDAV; whether it takes an app password the same
  way, how many previews to ask for at once without loading the server, and the cache's size
  limit.
- **Search**: WebDAV `SEARCH` covers names, types and dates; content search needs the full-text
  search app or the unified search, which not every server has.
- **Places that are not folders** (Recent, Shares, Tags, Favorites): their files come from
  searches and the sharing API, not from a folder's listing, so they have no folder ETag to
  cache by and no path to bookmark or to go up from; what the views, the path bar, the
  operations and the history do there (a path bar with one segment, "Shared with you", and
  Delete and Rename working on the files as they are), and how a share's file is opened when
  it is on another user's storage (its path as the sharer's, its id as ours).
- **Personal files and All files**: how the two differ on the servers Foldav is used with (a
  user's own files against everything mounted: shares, external storage, group folders), and
  whether one root is enough when the server has no difference.
- **File requests and Deleted shares**: the OCS calls behind them are newer than the rest of
  `files_sharing` (Nextcloud 32 for requests), so which versions have them and what the
  capabilities say about each.
- **Large folders**: listing thousands of files with `PROPFIND` (`Depth: 1`) at once versus in
  pages, and a view (`GtkGridView`, `GtkColumnView`) that stays fast with them.
- **Several accounts**: whether one window shows all accounts in the sidebar (as Notav's tree)
  and copies between accounts (a download and an upload).
