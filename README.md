<p align="center">
  <img src="foldav/linux/data/dev.aperte.Foldav.svg" width="128" alt="Foldav icon">
</p>

<h1 align="center">Foldav</h1>

<p align="center">A file manager for GNOME, made for the files in your Nextcloud.</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-GPL--3.0--or--later-blue" alt="License: GPL-3.0-or-later"></a>
  <a href="https://github.com/AperteDev/foldav/actions/workflows/ci.yml"><img src="https://github.com/AperteDev/foldav/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/AperteDev/foldav/releases/latest"><img src="https://img.shields.io/github/v/release/AperteDev/foldav" alt="Latest release"></a>
  <a href="https://aur.archlinux.org/packages/foldav-bin"><img src="https://img.shields.io/aur/version/foldav-bin" alt="AUR version"></a>
  <a href="https://copr.fedorainfracloud.org/coprs/selfref/foldav/"><img src="https://copr.fedorainfracloud.org/coprs/selfref/foldav/package/foldav/status_image/last_build.png" alt="COPR build status"></a>
</p>

Foldav (*folder* + DAV) is the sibling of [Todav](https://github.com/AperteDev/todav) and
[Notav](https://github.com/AperteDev/notav). It is at a very early stage: you can sign in
(GNOME Online Accounts, the browser or an app password), with more than one account, and browse
the folders of your Nextcloud Files. Everything else is planned: see the
[roadmap](docs/ROADMAP.md) and [features](docs/FEATURES.md).

Foldav is developed in my free time. If it helps you with your files, you can support its
development:

<p align="center">
  <a href="https://ko-fi.com/N5E229GJPZ"><img src="https://ko-fi.com/img/githubbutton_sm.svg" alt="Support me on Ko-fi"></a>
  <a href="https://github.com/sponsors/SelfRef"><img src="https://img.shields.io/badge/Sponsor-EA4AAA?style=for-the-badge&logo=githubsponsors&logoColor=white" alt="Sponsor on GitHub"></a>
  <a href="https://liberapay.com/SelfRef/donate"><img src="https://liberapay.com/assets/widgets/donate.svg" alt="Donate using Liberapay"></a>
</p>

## Getting started

Foldav needs GNOME 49 or newer. Download a package from the
[latest release](https://github.com/AperteDev/foldav/releases/latest), or install it from the
AUR (`foldav`, `foldav-bin` or `foldav-git`) or COPR (`dnf copr enable selfref/foldav`):

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

## Building

Needs Rust, GTK 4.14+, libadwaita 1.8+, libsecret, SQLite, `blueprint-compiler` and gettext.

```sh
cargo run -p foldav-linux      # or: make -C foldav/linux sandbox
cargo test --workspace
```

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).
