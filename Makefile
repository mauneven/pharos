PREFIX ?= /usr
DESTDIR ?=
APP_ID := com.mauneven.pharos

.PHONY: build test install uninstall clean

build:
	cargo build --locked --release

test:
	cargo test --locked

install:
	install -Dm755 target/release/pharos $(DESTDIR)$(PREFIX)/bin/pharos
	install -Dm755 target/release/pharos-window $(DESTDIR)$(PREFIX)/bin/pharos-window
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/icons/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/icons/$(APP_ID)-symbolic.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/pharos/LICENSE

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/pharos $(DESTDIR)$(PREFIX)/bin/pharos-window \
	      $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/pharos

clean:
	cargo clean

# Per-user install, no root: ~/.local/bin and ~/.local/share. The launcher
# entry points at the absolute binary path so it works even when ~/.local/bin
# isn't on the session PATH.
USER_PREFIX ?= $(HOME)/.local

.PHONY: install-user uninstall-user
install-user: build
	install -Dm755 target/release/pharos $(USER_PREFIX)/bin/pharos
	install -Dm755 target/release/pharos-window $(USER_PREFIX)/bin/pharos-window
	install -Dm644 data/icons/$(APP_ID).svg $(USER_PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/icons/$(APP_ID)-symbolic.svg $(USER_PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
	install -d $(USER_PREFIX)/share/applications
	sed 's|^Exec=pharos|Exec=$(USER_PREFIX)/bin/pharos|' data/$(APP_ID).desktop > $(USER_PREFIX)/share/applications/$(APP_ID).desktop
	-update-desktop-database -q $(USER_PREFIX)/share/applications 2>/dev/null
	-gtk-update-icon-cache -q -t $(USER_PREFIX)/share/icons/hicolor 2>/dev/null

uninstall-user:
	rm -f $(USER_PREFIX)/bin/pharos $(USER_PREFIX)/bin/pharos-window \
	      $(USER_PREFIX)/share/applications/$(APP_ID).desktop \
	      $(USER_PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg \
	      $(USER_PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg
