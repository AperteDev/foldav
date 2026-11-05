<p align="center">
  <img src="foldav/linux/data/dev.aperte.Foldav.svg" width="128" alt="Foldav icon">
</p>

<h1 align="center">Foldav</h1>

<p align="center">A file manager for GNOME, made for the files in your Nextcloud.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later"></a>
  <a href="https://github.com/AperteDev/foldav/releases/latest"><img src="https://img.shields.io/github/v/release/AperteDev/foldav" alt="Latest release"></a>
  <a href="https://github.com/AperteDev/foldav/releases"><img src="https://img.shields.io/github/downloads/AperteDev/foldav/total" alt="GitHub downloads"></a>
  <a href="https://aur.archlinux.org/packages/foldav-bin"><img src="https://img.shields.io/aur/version/foldav-bin" alt="AUR version"></a>
  <a href="https://copr.fedorainfracloud.org/coprs/selfref/foldav/"><img src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fcopr.fedorainfracloud.org%2Fapi_3%2Fpackage%3Fownername%3Dselfref%26projectname%3Dfoldav%26packagename%3Dfoldav%26with_latest_succeeded_build%3Dtrue&amp;query=%24.builds.latest_succeeded.source_package.version&amp;label=copr&amp;prefix=v&amp;color=51A2DA" alt="COPR version"></a>
</p>

Foldav (*folder* + DAV) is a file manager for the GNOME desktop that talks to your Nextcloud
directly. Nautilus reaches Nextcloud through GVfs's generic WebDAV backend, which knows nothing
of Nextcloud: copying or deleting a folder of many small files goes file by file and takes
minutes, files show no previews, and a deletion skips the trash bin. Foldav asks the server to
do what it can do in one request, and uses what Nextcloud's web Files offers: the trash bin,
versions, sharing, favorites. It looks and behaves like Files, and is the sibling of
[Todav](https://github.com/AperteDev/todav), the tasks and calendar app, and
[Notav](https://github.com/AperteDev/notav), the notes app.

Foldav is at an early stage: browsing, favorites, the operations on the server, transfers,
thumbnails, the trash bin, versions, sharing, tags and search by name work; what is left is in
the [roadmap](docs/ROADMAP.md).

Foldav is developed in my free time. If it helps you with your files, you can support its
development:

<p align="center">
  <a href="https://ko-fi.com/N5E229GJPZ"><img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="Support me on Ko-fi"></a>
  <a href="https://github.com/sponsors/SelfRef"><img src="https://img.shields.io/badge/Sponsor-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor on GitHub"></a>
  <a href="https://liberapay.com/SelfRef/donate"><img src="https://liberapay.com/assets/widgets/donate.svg" alt="Donate using Liberapay"></a>
</p>

## Features

- The folders of your Nextcloud Files as in Files: icons, list or columns (one per folder, as in
  the Files extension "My Computer"), a path bar, back and forward, hidden files on request.
- A sidebar with your accounts, favorite folders and a folder tree.
- Favorites shared with Nextcloud Files: star files and folders, and they show on all your
  devices and in the web app.
- Quick to open: a folder shows at once from what was listed before, kept on this device, then
  is checked against the server by its ETag, so a folder that did not change costs one small
  request.
- New folder, rename, cut, copy, paste and delete, each one request to the server whatever the
  size of the folder; deletions go to the trash bin, and Ctrl+Z takes back a rename, move or
  deletion.
- Open a file in its app and what you save goes back; upload and download files and folders
  (big files in chunks, small ones many to a request, a folder as one ZIP), with drag and drop
  inside the window and to and from Files; long actions listed at the foot of the sidebar.
- The web app's places: Recent, Starred, shares, tags and the trash bin; thumbnails; search by
  name; versions, public links and shares with users, and tags on a file.
- Sign-in through GNOME Online Accounts, the browser, or a login name and app password; more
  than one account.
- Settings synced between your devices.

All of what Foldav does, and what it will, is listed in [FEATURES.md](docs/FEATURES.md).

## Getting started

Foldav needs GNOME 49 or newer. Download a package from the
[latest release](https://github.com/AperteDev/foldav/releases/latest), or install it from the
AUR (`foldav`, `foldav-bin` or `foldav-git`) or COPR (`dnf copr enable selfref/foldav`):

<p align="center">
  <a href="https://github.com/AperteDev/foldav/releases/latest"><img src="https://raw.githubusercontent.com/Kunzisoft/Github-badge/main/get-it-on-github.png" alt="Get it on GitHub" height="60"></a>
</p>

```sh
# Flatpak, on any distribution (the GNOME runtime comes from Flathub)
flatpak install --user dev.aperte.Foldav.flatpak

# Ubuntu 26.04 and newer
sudo apt install ./foldav_<version>_amd64.deb

# Other distributions: the same files to unpack into /
sudo tar -xzf foldav-<version>-x86_64.tar.gz -C /
```

Then open Foldav and sign in to your Nextcloud.

## Translations

Foldav speaks English and Polish, and follows your GNOME language. Missing your language? Ask for
it in an [issue](https://github.com/AperteDev/foldav/issues), or translate it yourself (see
[CONTRIBUTING.md](docs/CONTRIBUTING.md#translations)).

## Development

### Building and installing

**Dependencies:** Rust (stable), GTK 4.14+, libadwaita 1.8+, libsecret, SQLite,
`blueprint-compiler`, `glib-compile-resources` (GLib) and gettext's `msgfmt`.

```sh
# The app: binary, desktop file and icons in ~/.local
make -C foldav/linux install

# Or run it from the source tree
cargo run -p foldav-linux

# A separate copy with its own data, for trying other accounts (see AGENTS.md)
make -C foldav/linux sandbox
```

A Flatpak manifest is included too: `make -C foldav/linux flatpak` (needs `flatpak-builder`). For
Fedora, `foldav/linux/rpm/foldav.spec` builds an RPM; COPR builds it with `.copr/Makefile`, which
also works locally:

```sh
make -f .copr/Makefile srpm outdir=$PWD/target/rpm spec=$PWD/foldav/linux/rpm/foldav.spec
```

### Tests

```sh
cargo test --workspace
```

The tests against a server need a Nextcloud account: put its `FOLDAV_URL`, `FOLDAV_USER` and
`FOLDAV_PASS` in a `.env` file and run
`cargo test -p foldav-core --features integration`. Each test works in its own
`it-foldav/<name>/` folder.

Every push is checked and packaged by [CI](.github/workflows/ci.yml); releases are made by the
manual [Release](.github/workflows/release.yml) workflow and listed in the
[changelog](docs/CHANGELOG.md). See [CONTRIBUTING.md](docs/CONTRIBUTING.md) before sending a
pull request.

## License

[GPL-3.0-or-later](LICENSE)
