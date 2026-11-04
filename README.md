<p align="center">
  <img src="foldav/linux/data/dev.aperte.Foldav.svg" width="128" alt="Foldav icon">
</p>

<h1 align="center">Foldav</h1>

<p align="center">A file manager for GNOME, made for the files in your Nextcloud.</p>

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

## Building

Needs Rust, GTK 4.14+, libadwaita 1.7+, libsecret, SQLite, `blueprint-compiler` and gettext.

```sh
cargo run -p foldav-linux      # or: make -C foldav/linux sandbox
cargo test --workspace
```

## License

GPL-3.0-or-later, see [LICENSE](LICENSE).
