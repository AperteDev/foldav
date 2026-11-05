#!/bin/sh
# Package a staged tree (`make -C foldav/linux stage`, the tarball's content) as a .deb. Run it on
# the Ubuntu release the package is for: dpkg-shlibdeps takes the dependencies from the libraries
# the binary links to, as that release names them.
#
#   scripts/deb.sh <staged tree> <version> <out dir>
set -eu

stage=$1 version=$2 out=$3
arch=$(dpkg --print-architecture)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

cp -a "$stage" "$work/root"
# dpkg-shlibdeps reads a source package's control file from the current directory.
mkdir -p "$work/src/debian"
printf 'Source: foldav\n\nPackage: foldav\nArchitecture: any\n' > "$work/src/debian/control"
# libadwaita's version comes from the widgets the UI files use (Adw.ShortcutsDialog), which
# linking does not show: below, in place of the one found.
depends=$(cd "$work/src" && dpkg-shlibdeps -O "$work/root/usr/bin/foldav-linux" |
  sed -n 's/^shlibs:Depends=//p' | sed 's/libadwaita-1-0 ([^)]*), *//; s/, *libadwaita-1-0 ([^)]*)//')

mkdir -p "$work/root/DEBIAN"
cat > "$work/root/DEBIAN/control" <<EOF
Package: foldav
Version: $version
Architecture: $arch
Maintainer: SelfRef <git@selfref.dev>
Installed-Size: $(du -sk "$work/root" | cut -f1)
Depends: $depends, libadwaita-1-0 (>= 1.8)
Recommends: gnome-keyring
Section: gnome
Priority: optional
Homepage: https://github.com/AperteDev/foldav
Description: A file manager for GNOME, made for the files in your Nextcloud
 Foldav is a file manager for the GNOME desktop. It shows the folders and
 files of your Nextcloud as Files does, and copies, moves and deletes folders
 on the server in one request, not file by file.
EOF

mkdir -p "$out"
dpkg-deb --root-owner-group --build "$work/root" "$out/foldav_${version}_$arch.deb"
