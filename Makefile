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
	install -Dm644 LICENSE $(DESTDIR)$(PREFIX)/share/licenses/beacon/LICENSE

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/beacon \
	      $(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop \
	      $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -rf $(DESTDIR)$(PREFIX)/share/licenses/beacon

clean:
	cargo clean
