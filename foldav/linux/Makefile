# Per-user install for development; `make flatpak` builds and installs the Flatpak instead.
PREFIX ?= $(HOME)/.local
BINDIR = $(PREFIX)/bin
DATADIR = $(PREFIX)/share
ICONS = $(DATADIR)/icons/hicolor
LOCALE = $(DATADIR)/locale
ID = dev.aperte.Foldav
# Translations (see scripts/i18n.sh); po/its says what of the metainfo is translated.
PO = ../../po
export GETTEXTDATADIRS = $(abspath $(PO))
# The repository's workspace: from here Cargo would find the bindings' one (foldav/Cargo.toml).
CARGO_BUILD = cargo build --manifest-path ../../Cargo.toml

install:
	$(CARGO_BUILD) --release -p foldav-linux
	$(MAKE) install-data

# Binary and data files of an already built release; the Flatpak build uses this too. DESTDIR
# stages them for a package. The desktop file and metainfo get their translations; the catalogs
# go where the app looks for them (../share/locale from its binary).
install-data:
	install -Dm755 ../../target/release/foldav-linux $(DESTDIR)$(BINDIR)/foldav-linux
	mkdir -p $(DESTDIR)$(DATADIR)/applications $(DESTDIR)$(DATADIR)/metainfo
	msgfmt --desktop --template=data/$(ID).desktop -d $(PO) \
	  -o $(DESTDIR)$(DATADIR)/applications/$(ID).desktop
	msgfmt --xml --template=data/$(ID).metainfo.xml -d $(PO) \
	  -o $(DESTDIR)$(DATADIR)/metainfo/$(ID).metainfo.xml
	for lang in $$(grep -v '^#' $(PO)/LINGUAS); do \
	  mkdir -p $(DESTDIR)$(LOCALE)/$$lang/LC_MESSAGES; \
	  msgfmt -o $(DESTDIR)$(LOCALE)/$$lang/LC_MESSAGES/foldav.mo $(PO)/$$lang.po; \
	done
	install -Dm644 data/$(ID).svg $(DESTDIR)$(ICONS)/scalable/apps/$(ID).svg
	install -Dm644 data/$(ID)-symbolic.svg $(DESTDIR)$(ICONS)/symbolic/apps/$(ID)-symbolic.svg

# The system-wide tree the tarball and the .deb are made of, in DESTDIR:
#   make stage PREFIX=/usr DESTDIR=$PWD/../../target/stage
stage:
	$(CARGO_BUILD) --release --locked -p foldav-linux
	$(MAKE) install-data

# Needs flatpak-builder (or the org.flatpak.Builder Flatpak); the build fetches crates over the
# network. Flathub builds offline: add a generated cargo-sources.json when submitting.
FLATPAK_BUILDER ?= $(shell command -v flatpak-builder || echo flatpak run org.flatpak.Builder)
flatpak:
	$(FLATPAK_BUILDER) --user --install --force-clean --install-deps-from=flathub \
	  --state-dir=../../target/flatpak-builder ../../target/flatpak flatpak/$(ID).json

# GTK's Broadway backend draws into a web page (see AGENTS.md for its limits). The page opens
# once the app has drawn: a browser connecting earlier aborts broadwayd. Animations are off
# (GTK settings in target/broadway). A running instance is quit first: the app is
# single-instance, so a new launch would only show the existing window. broadwayd stops with
# the app.
broadway:
	$(CARGO_BUILD) -p foldav-linux
	busctl --user --auto-start=no call $(ID) /dev/aperte/Foldav org.gtk.Actions Activate 'sava{sv}' quit 0 0 2>/dev/null || true
	for i in $$(seq 50); do busctl --user status $(ID) >/dev/null 2>&1 || break; sleep 0.1; done
	mkdir -p ../../target/broadway/gtk-4.0
	printf '[Settings]\ngtk-enable-animations=false\n' > ../../target/broadway/gtk-4.0/settings.ini
	gtk4-broadwayd :6 & b=$$!; trap 'kill $$b' EXIT; sleep 0.5; \
	  XDG_CONFIG_HOME=$(CURDIR)/../../target/broadway GDK_BACKEND=broadway BROADWAY_DISPLAY=:6 \
	  ../../target/debug/foldav-linux & a=$$!; \
	  sleep 2; chromium --app=http://127.0.0.1:8086 >/dev/null 2>&1 & \
	  wait $$a

# A separate copy for testing other connections: own data in target/sandbox (kept between
# runs; sandbox-reset wipes it), own keyring entries, runs next to the normal app.
sandbox:
	$(CARGO_BUILD) -p foldav-linux
	FOLDAV_SANDBOX=1 XDG_DATA_HOME=$(CURDIR)/../../target/sandbox ../../target/debug/foldav-linux

sandbox-reset:
	rm -rf ../../target/sandbox

uninstall:
	rm -f $(BINDIR)/foldav-linux $(DATADIR)/applications/$(ID).desktop \
	  $(DATADIR)/metainfo/$(ID).metainfo.xml \
	  $(ICONS)/scalable/apps/$(ID).svg $(ICONS)/symbolic/apps/$(ID)-symbolic.svg \
	  $(LOCALE)/*/LC_MESSAGES/foldav.mo

.PHONY: install install-data stage flatpak uninstall broadway sandbox sandbox-reset
