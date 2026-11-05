# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Packages of each release: a Flatpak, a tarball, a `.deb`, Fedora RPMs on COPR and the AUR
  packages `foldav`, `foldav-git` and `foldav-bin`; a new app icon
- Folders open at once: what was listed of each is kept on this device and shown first, then
  the server is asked in the background; by the folder's ETag, so a folder that did not change
  costs one small request, not a listing of everything in it. The tree and the columns fill
  from it too, also after a restart
- A window as Nautilus': the files as icons, a list (sorted by clicking a column's header) or
  columns, one per folder from the root, switched in the header or with Ctrl+1 to 3; back and
  forward (Alt+Left, Alt+Right) and the folder above (Alt+Up); a path bar with a segment for
  each folder; Show Hidden Files (Ctrl+H)
- The columns view as the Files extension "My Computer" has it: rows as there, the selection in
  the column you work in in the accent colour and the way to it grey; a preview column for a
  file (its icon, name, type, size and date); the columns resized by dragging their edge, and
  the width kept; Left and Right between columns; a click on a column's empty room closes the
  columns after it
- Favorites, as in Nextcloud Files: a star on files and folders (Add to Favorites in the
  right-click menu, the path bar's menu for the folder shown, Ctrl+D), the favorite folders
  listed in the sidebar (right click removes one). They are the server's, so they show on all
  your devices and in the web app, kept here and sent when the server can be reached; what is
  starred elsewhere shows when the window comes to the front or on a reload
- A sidebar with the accounts (only with more than one; changing the account keeps the folder
  shown where the new one has it), the favorite folders of the account and a tree of its
  folders
- Open a file (double click, Enter or Ctrl+O) in its default app, from a copy kept on this
  device in the cache folder: used again while the server says the file is the same (one small
  request), read-only, and deleted when not used for a month. Download files and folders
  (Download… in the right-click menu) to a folder you pick, none overwritten
- Long actions show as in Files: once an action has taken a second, a row with a ring and its
  title is added at the foot of the sidebar; a click opens its progress, the time left and the
  speed, and a way to cancel a transfer
- Upload files and folders (Upload Files… with Ctrl+U, Upload Folder… in the right-click menu
  and the path bar's): a file in one request, a big one (32 MB or more) in chunks of 16 MB
  that are sent again one at a time when the connection drops, with the file's date kept;
  when a name is taken you choose Replace (the server keeps the old file as a version), Keep
  Both or Skip
- The places of Nextcloud Files in the sidebar, as the web app has them (each only where the
  server has it): Recent, Starred (all that is starred, files too), Shared with You, Shared
  with Others, Shared by Link, Tags (each tag opens the files that have it) and Deleted
  Files, and the storage used at the foot
- Thumbnails: images, documents and videos show the preview the server made of them instead
  of the type's icon, in the icons, the list and the columns' preview, fetched a few at a
  time while you scroll and kept on this device by the file's ETag (deleted after a month
  unused)
- The trash bin: Deleted Files lists what was deleted; Restore, Delete Permanently and
  Empty Trash, each asking first where it cannot be taken back
- Search by name in the folder shown and below it (Ctrl+F or the magnifier in the header),
  searching on the server as you type; Escape leaves it
- A change another app saves to the copy of an opened file goes back to the server (also one
  made while Foldav was closed, at the next start), unless the file changed on the server
  meanwhile: then it stays, and your version is kept beside it as "name (conflicted
  copy).ext", as the desktop client does
- Drag and drop as in Files: files to a folder in the views, the sidebar or the path bar move
  there (Ctrl copies); files dragged from Files or the desktop into the window are uploaded
  to the folder they are dropped on or shown; files dragged out of the window to Files or the
  desktop are downloaded first, listed in the sidebar like any transfer
- New folder (Ctrl+Shift+N), rename (F2), cut, copy and paste (a copy of a name taken is
  named "a (copy)"), and delete to the trash bin, each one request to the server whatever the
  size of the folder, from the keys, the header or a right click; several files selected at
  once; Ctrl+Z, or Undo on the toast, takes back a rename, move or deletion
- The first version of the Linux app: sign in with GNOME Online Accounts, the browser or a login
  name and app password, with more than one account; the folders and files of each account's
  Nextcloud Files, opened by a click, with Alt+Up to the parent folder; Preferences (style,
  accounts, settings synced between devices); a Polish translation

[Unreleased]: https://github.com/AperteDev/foldav/commits/HEAD
