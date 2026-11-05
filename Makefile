PREFIX ?= /usr
DESTDIR ?=
APP_ID := com.mauneven.beacon

.PHONY: build test install uninstall clean

build:
	cargo build --locked --release

test:
	cargo test --locked

install:
	install -Dm755 target/release/beacon $(DESTDIR)$(PREFIX)/bin/beacon
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop
	install -Dm644 data/icons/$(APP_ID).svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 -t $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps data/icons/$(APP_ID)-symbolic.svg data/icons/$(APP_ID)-paused-symbolic.svg
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/beacon/LICENSE

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/beacon \
	      $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-paused-symbolic.svg
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/beacon

clean:
	cargo clean

# Per-user install, no root: ~/.local/bin and ~/.local/share. The launcher
# entry points at the absolute binary path so it works even when ~/.local/bin
# isn't on the session PATH.
USER_PREFIX ?= $(HOME)/.local

.PHONY: install-user uninstall-user
install-user: build
	install -Dm755 target/release/beacon $(USER_PREFIX)/bin/beacon
	install -Dm644 data/icons/$(APP_ID).svg $(USER_PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 -t $(USER_PREFIX)/share/icons/hicolor/symbolic/apps data/icons/$(APP_ID)-symbolic.svg data/icons/$(APP_ID)-paused-symbolic.svg
	install -d $(USER_PREFIX)/share/applications
	sed 's|^Exec=beacon|Exec=$(USER_PREFIX)/bin/beacon|' data/$(APP_ID).desktop > $(USER_PREFIX)/share/applications/$(APP_ID).desktop
	-update-desktop-database -q $(USER_PREFIX)/share/applications 2>/dev/null
	-gtk-update-icon-cache -q -t $(USER_PREFIX)/share/icons/hicolor 2>/dev/null

uninstall-user:
	rm -f $(USER_PREFIX)/bin/beacon \
	      $(USER_PREFIX)/share/applications/$(APP_ID).desktop \
	      $(USER_PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg \
	      $(USER_PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-symbolic.svg \
	      $(USER_PREFIX)/share/icons/hicolor/symbolic/apps/$(APP_ID)-paused-symbolic.svg
