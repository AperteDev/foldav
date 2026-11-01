# Foldav — Roadmap

Planned work, not yet done; the order is not fixed. Nothing is built yet: this is the plan.

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
- Tooling and releases as in Todav and Notav: CI on every push (fmt, clippy with warnings as
  errors, unit tests, packages), the manual Release workflow, Keep a Changelog, the lowest
  supported GNOME 49.

## Code from Todav and Notav

Copied, not shared, as Notav did from Todav; the copies may diverge.

| Piece | From | Notes |
| --- | --- | --- |
| Login Flow v2, app password sign-in, minimal JSON | Todav, Notav (`login.rs`, `json.rs`) | As is |
| WebDAV XML, multistatus, percent-encoding, requests with retries | Notav `core/src/dav.rs` | Plus `COPY`, `MOVE`, `SEARCH`, chunked and bulk uploads |
| Accounts, synced settings, `Error`, `Client::open`, store migrations, `kv` | Notav `core/src/account.rs`, `settings.rs`, `lib.rs`, `store.rs` | Nextcloud accounts only (no local one) |
| Favorites, trash bin, versions, server features | Notav `core/src/files.rs` | For any file, not only notes |
| GNOME Online Accounts, gettext, Secret Service, adding accounts, Preferences, About, update banner | Notav `linux/app/src/` | As is, adapted |
| Build, packaging, release | Notav (`build.rs`, `linux/Makefile`, Flatpak, `scripts/`, `.github/workflows/`, RPM, COPR) | No GtkSourceView |

Once a third app copies the same code, a shared crate in its own repository becomes worth it
(see Notav's roadmap, "Later").

## Planned features

1. **Browsing**: a folder's files as a grid of thumbnails or a list (name, size, modified,
   shared), sorted and filtered; a path bar; the sidebar as the web app's (All files, Recent,
   Favorites, Shared with me, Shared by me, Tags, Trash) and one entry per account. Thumbnails
   from the server's previews (`/core/preview`, for files with `has-preview`), cached by ETag.
2. **Operations on the server**: new folder, rename, copy, move and delete with one WebDAV
   request each, whatever the folder's size; a deletion goes to the trash bin when the server
   has it. Progress and errors in a toast or a panel; Ctrl+Z undoes a rename, move or deletion
   (Notav's `undo.rs`).
3. **Transfers**: uploads (chunked for big files, Nextcloud's bulk upload for many small ones)
   and downloads, dragged to and from Nautilus or the desktop; a folder downloaded as one ZIP
   where the server offers it. Transfers queue, resume after a dropped connection, and finish
   before the app quits.
4. **Opening files**: a file opens in its default app from a local copy; a change saved there
   is uploaded back (with the ETag checked first: a conflict keeps both, as the desktop client
   does).
5. **Nextcloud's features**:
   - Trash: restore, delete for good, empty it.
   - Versions: list a file's versions, restore one, open one.
   - Sharing: public links (password, expiry, permissions), shares with users, groups and
     federated users; copy the link (OCS `files_sharing` API).
   - Favorites, tags (`systemtags`), recent files, search by name and content (WebDAV `SEARCH`,
     the unified search for content).
   - "Open in the browser" for what Foldav does not do (comments, activity, apps' own views).
6. **Change notifications**: Nextcloud's Client Push (`notify_push`) refreshes an open folder
   when its files change elsewhere; polling of folder ETags otherwise (as Notav plans).

Later (v2):

- Image and PDF viewer in the app (with the previews already cached); media streamed rather
  than downloaded first.
- Links from Notav and Todav to files (`foldav://`), and Notav's attachments in Foldav.
- Offline folders: chosen folders kept on the device and synced, for those without the
  desktop client.

## Open questions and risks

- **Opening files in other apps**: a local copy in a cache folder and watching it for saves is
  simplest; a FUSE mount or a GVfs backend would let apps open the server's paths directly but
  is much more work. Also how long copies are kept and when they are cleaned.
- **Bulk operations**: which Nextcloud versions have bulk upload (`/remote.php/dav/bulk`) and
  ZIP downloads of folders over WebDAV, and what the fallbacks are; whether a server-side `COPY`
  of a huge folder can time out on the client while the server is still working.
- **Thumbnails**: the preview endpoint is not WebDAV; whether it takes an app password the same
  way, how many previews to ask for at once without loading the server, and the cache's size
  limit.
- **Search**: WebDAV `SEARCH` covers names, types and dates; content search needs the full-text
  search app or the unified search, which not every server has.
- **Large folders**: listing thousands of files with `PROPFIND` (`Depth: 1`) at once versus in
  pages, and a view (`GtkGridView`, `GtkColumnView`) that stays fast with them.
- **Several accounts**: whether one window shows all accounts in the sidebar (as Notav's tree)
  and copies between accounts (a download and an upload).
