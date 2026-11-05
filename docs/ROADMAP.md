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
   path bar, history, favorite folders, folder tree and accounts is done): thumbnails from the server's
   previews (`/core/preview`, for files with `has-preview`), cached by ETag; a limit to the
   listing cache's size, and a long-offline mode that shows it without asking; the header's
   search; a zoom for the icons; sorting also in the icons and columns; the last folder and
   account restored at start.
2. **The web app's places in the sidebar**, what is left of them (All files is the tree's
   "Home"; Recent, Starred, Shared with you, Shared with others, Shared by link, Tags and
   Deleted files are built, each shown where the server's capabilities have it, and the
   storage used): *Personal files*: only the user's own, without what is shared with them and
   external storage; *Pending shares* (`shares/pending`, to accept or decline), *File
   requests* (upload-only links, Nextcloud 32), *Deleted shares* (`deletedshares`, to
   restore); *Files settings* (Preferences: sorting, hidden files, the web app's own settings
   in the browser). The places' files come in the server's order, whatever the view sorts by.
3. **Operations on the server**, what is left of them (new folder, rename, copy, move, delete,
   Ctrl+Z and dragging files inside the window are done): a
   deletion asks nothing and offers Undo, but undoing needs the trash bin, so check the
   server's capabilities (`Features`, as in Notav) and ask first where it has none; a
   choice between replacing and keeping both when
   a name is taken in a move or copy (now a move refuses and a copy takes a free name); copies
   between accounts (a drag from one account to another is refused now).
4. **Transfers**, what is left of them (downloads, uploads in one request or in chunks, drag
   and drop inside the window and to and from other apps, and the sidebar's rows for running
   actions and their cancel, are done): Nextcloud's bulk upload for many small files; a folder
   downloaded as one ZIP where the server offers it (now file by file). Transfers queue,
   resume after a dropped connection (a chunk is sent again, a download starts over), and
   finish before the app quits (now closing the window stops them). A stalled connection is
   not given up on until the system does (`ureq` has no idle timeout). Dragging to and from
   other apps is not tested against Files itself, only between the window's own views.
5. **Opening files**, what is left of it (a file opens in its default app from a copy kept in
   the cache folder, checked against the server's ETag, deleted after a month unused; a
   change saved to the copy is uploaded back, with the ETag checked first and a conflict
   kept beside the file): Open With…; several files opened at once; a limit to the copies'
   total size; a copy of a file renamed or moved on the server meanwhile is sent to its old
   path (as a conflicted copy, the ETag check fails there).
6. **Nextcloud's features** on files (the places that list them are above; the trash bin,
   restoring, deleting for good and emptying it are built):
   - Versions: list a file's versions, restore one, open one.
   - Sharing: public links (password, expiry, permissions), shares with users, groups and
     federated users; copy the link (OCS `files_sharing` API).
   - Tags: tag a file, make and remove tags (`systemtags`); the tags and their files are
     listed already.
   - Search by content (the unified search); by name is built (Ctrl+F, WebDAV `SEARCH`).
   - "Open in the browser" for what Foldav does not do (comments, activity, apps' own views).
7. **Change notifications**: Nextcloud's Client Push (`notify_push`) refreshes an open folder
   when its files change elsewhere; polling of folder ETags otherwise (as Notav plans).

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
- **Bulk operations**: which Nextcloud versions have bulk upload (`/remote.php/dav/bulk`) and
  ZIP downloads of folders over WebDAV, and what the fallbacks are; whether a server-side `COPY`
  of a huge folder can time out on the client while the server is still working.
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
