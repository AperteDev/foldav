# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A window as Nautilus': the files as icons, a list (sorted by clicking a column's header) or
  columns, one per folder from the root, switched in the header or with Ctrl+1 to 3; back and
  forward (Alt+Left, Alt+Right) and the folder above (Alt+Up); a path bar with a segment for
  each folder; Show Hidden Files (Ctrl+H)
- A sidebar with the accounts (only with more than one; changing the account keeps the folder
  shown where the new one has it), the bookmarks of the account (a row, as in Files, bookmarks
  the folder shown; right click removes one) and a tree of its folders
- New folder (Ctrl+Shift+N), rename (F2), cut, copy and paste (a copy of a name taken is
  named "a (copy)"), and delete to the trash bin, each one request to the server whatever the
  size of the folder, from the keys, the header or a right click; several files selected at
  once; Ctrl+Z, or Undo on the toast, takes back a rename, move or deletion
- The first version of the Linux app: sign in with GNOME Online Accounts, the browser or a login
  name and app password, with more than one account; the folders and files of each account's
  Nextcloud Files, opened by a click, with Alt+Up to the parent folder; Preferences (style,
  accounts, settings synced between devices); a Polish translation
