# Foldav — Features

What Foldav can do, in short. The details of each release are in the
[changelog](CHANGELOG.md), the plans in the [roadmap](ROADMAP.md).

## Now

- Sign in with GNOME Online Accounts, the browser, or a login name and app password; more than
  one account.
- Browse the folders of your Nextcloud Files as in Files: icons, list or columns, a path bar,
  back and forward, a sidebar with the accounts, favorite folders and a folder tree; hidden files
  shown on request; the view menu as Files': the icon size (also Ctrl+plus, Ctrl+minus,
  Ctrl+0), the sort (A-Z, Z-A, Last or First Modified, Size, Type) for all three views, and
  the captions beneath the icons' names (size, type, date).
- Type a folder's path in the location (Ctrl+L, or a click on the path bar's room).
- Select several files by dragging a rectangle around them; a pill at the foot says what is
  selected and its size.
- Tabs as in Files, each with its folder and history; folders open in a new tab by a middle
  click or from the right-click menu; files dragged onto a tab go into its folder.
- Favorites, shared with Nextcloud Files: star files and folders, the favorite folders in the
  sidebar; kept on the device and synced with the server.
- Folders show at once from what was listed before (kept on this device), then are checked
  against the server by their ETag.
- New folder, rename, cut, copy, paste and delete on the server, one request each whatever the
  folder's size; deletions go to the trash bin; undo for renames, moves and deletions.
- Open a file in its default app from a copy kept on the device and checked against the server
  each time; download files and folders to a folder you pick; long actions listed at the foot
  of the sidebar with their progress and time left, transfers cancellable.
- Upload files and folders, big files in chunks that survive a dropped connection; choose
  Replace, Keep Both or Skip when a name is taken.
- Download a folder or several files as one ZIP; copy files to another account by dragging
  them onto it; the folder shown follows changes made on other devices.
- Share files and folders by public link (password, expiry) or with users and groups; see
  and restore a file's versions; tag files.
- Thumbnails of images, documents and videos, from the server's previews, kept on the device
  (up to 256 MB, the oldest going first); the columns view's preview shows an image or a PDF's
  first page large and a text file's start (Preferences can turn it off).
- The web app's places: Recent, Starred, Shared with You and Others, Shared by Link, Tags;
  the trash bin with restore, delete for good and empty; the storage used, with a bar.
- Search by name (Ctrl+F), done by the server.
- Edit a file in another app: what you save goes back to the server; a file changed there
  meanwhile is not overwritten, your version is kept as a "conflicted copy".
- Drag and drop: inside the window (move, Ctrl copies), in from Files or the desktop (upload)
  and out to them (download).
- Settings synced between your devices, through the account you choose.

## Planned

Everything in the [roadmap](ROADMAP.md): operations on the server, transfers, thumbnails,
trash, versions, sharing, favorites, search, change notifications.
