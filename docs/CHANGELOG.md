# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
