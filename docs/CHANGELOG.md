# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-11-06

### Added

- Open a file (double click, Enter or Ctrl+O) in its default app, from a copy kept on this
  device in the cache folder: used again while the server says the file is the same (one small
  request), read-only, and deleted when not used for a month. Download files and folders
  (Download… in the right-click menu) to a folder you pick, none overwritten
- Long actions show as in Files: once an action has taken a second, a row with a ring and its
  title is added at the foot of the sidebar; a click opens its progress, the time left and the
  speed, and a way to cancel a transfer
- Upload files and folders (Upload Files… with Ctrl+U, Upload Folder… in the right-click menu
  and the path bar's): a file in one request, a big one (32 MB or more) in chunks of 16 MB,
  three at a time, each sent again when the connection drops, and small files (256 KB or
  less) a hundred to a request with Nextcloud's bulk upload, with the files' dates kept;
  when a name is taken you choose Replace (the server keeps the old file as a version), Keep
  Both or Skip
- The places of Nextcloud Files in the sidebar, as the web app has them (each only where the
  server has it): Recent, Starred (all that is starred, files too), Shared with You, Shared
  with Others, Shared by Link, Tags (each tag opens the files that have it) and Deleted
  Files, and the storage used at the foot ("Used x out of y", with a thin bar that turns red
  when a tenth or less is left)
- Open in Browser (right-click menu): the file in Nextcloud's web app, for what Foldav does not
  do (comments, activity, apps' own views)
- Open With… (right-click menu) picks the app for a file; Enter opens all the files selected
- Pasting or dropping files where a name is taken asks what to do: Replace, Keep Both or
  Skip (a copy into its own folder is still "a (copy)")
- Download as ZIP… (right-click menu): a folder or several files as one ZIP the server makes,
  in one request however many files
- Files dragged onto another account in the sidebar are copied to its root (by way of this
  device)
- The folder shown is checked for changes made elsewhere every 30 seconds and when the window
  comes to the front (one small request), and shows them without losing your place
- Share… in the right-click menu (where the server has sharing): the file's public links to
  copy or take down, made with an optional password, an expiry date and editing allowed (a new
  link is copied at once), and the users and groups it is shared with, found by name, who
  can be allowed to edit or removed
- Versions… (where the server keeps them): a file's earlier versions with their date and
  size, each opened in its app or put back as the file
- Tags… (where the server has them): check the tags a file has, search the tags, make a new one
- Thumbnails: images, documents and videos show the preview the server made of them instead
  of the type's icon, in the icons, the list and the columns' preview, fetched a few at a
  time while you scroll and kept on this device by the file's ETag (deleted after a month
  unused)
- The location as a path to type, as in Files: a click on the path bar's room or Ctrl+L turns
  the bar into a field with the folder's path; Enter goes there (a toast says when there is no
  such folder), Escape or a click elsewhere brings the bar back; dragging the bar still moves
  the window
- The Tags place shows how many files each tag is on in the size column (counted in the
  background after the list shows, a few requests at a time), and no date for a tag
- The star of a favorite as in Nextcloud's web app: filled, toned (a dark gold on light
  themes, a soft yellow on dark), at the corner of the file's icon in the icons, after the name
  in the list and the columns, in the same colour in all three
- What is selected, said in a pill at the foot of the icons and the list, as in Files: a file
  by its name and size, several by their number and together size (folders and other items
  counted apart); a click on the room between the files selects none
- Select by dragging a rectangle on the room between the files, in the icons, the list and each
  column of the columns view
- Tabs, as in Files: Ctrl+T opens one at the folder shown, Ctrl+W closes it (the last one
  closes the window), Ctrl+Page Down and Ctrl+Page Up (or Ctrl+Tab) switch, Ctrl+Shift+Page
  Up and Down move a tab, Alt+1 to 9 pick one; a folder opens in a new tab from "Open in New
  Tab" in the right-click menu or a middle click (on the views, the sidebar and the path
  bar). Each tab has its own folder and history, and files dragged onto a tab are moved into
  its folder (Ctrl: copied; from other apps: uploaded)
- The view menu as Files': Icon Size with plus and minus (Ctrl+plus, Ctrl+minus, Ctrl+0; the
  icons' and the list's own), Sort (A-Z, Z-A, Last Modified, First Modified, Size, Type; one
  for the icons, the list and the columns, the list's header sets it too), Show Hidden Files,
  and Captions…: the size, type or date of a file beneath its name in the icons, up to three,
  more of them as the icons grow
- A bigger preview of the file selected in the columns view: an image (or a document or
  video or a PDF's first page, from the server's preview) shown large and the start of a text
  file, as in Files; "Preview Files" in Preferences, on to begin with, turns it off. What is
  fetched for it is kept on this device by the file's ETag, never asked for twice, and the
  oldest previews go when they take more than 256 MB
- The trash bin: Deleted Files lists what was deleted; Restore, Delete Permanently and
  Empty Trash, each asking first where it cannot be taken back
- Search by name in the folder shown and below it, as in Files (Ctrl+F or the button with the
  loupe in the header): the field takes the path bar's place, the server is asked a
  moment after each key (the results stay on screen with a "Searching…" pill meanwhile), each
  result says under its name the folder it is in (so do Recent, Starred and the other places),
  back, forward and up are off while the results show, and Escape or the button leaves it
- A change another app saves to the copy of an opened file goes back to the server (also one
  made while Foldav was closed, at the next start), unless the file changed on the server
  meanwhile: then it stays, and your version is kept beside it as "name (conflicted
  copy).ext", as the desktop client does
- Drag and drop as in Files: files to a folder in the views, the sidebar or the path bar move
  there (Ctrl copies); files dragged from Files or the desktop into the window are uploaded
  to the folder they are dropped on or shown; files dragged out of the window to Files or the
  desktop are downloaded first, listed in the sidebar like any transfer

## [0.1.0] - 2026-11-05

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
- New folder (Ctrl+Shift+N), rename (F2), cut, copy and paste (a copy of a name taken is
  named "a (copy)"), and delete to the trash bin, each one request to the server whatever the
  size of the folder, from the keys, the header or a right click; several files selected at
  once; Ctrl+Z, or Undo on the toast, takes back a rename, move or deletion
- The first version of the Linux app: sign in with GNOME Online Accounts, the browser or a login
  name and app password, with more than one account; the folders and files of each account's
  Nextcloud Files, opened by a click, with Alt+Up to the parent folder; Preferences (style,
  accounts, settings synced between devices); a Polish translation

[Unreleased]: https://github.com/AperteDev/foldav/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/AperteDev/foldav/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/AperteDev/foldav/releases/tag/v0.1.0
