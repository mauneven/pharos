# Maintainer: mauneven <mauneven@gmail.com>
pkgname=pharos
pkgver=0.2.0
pkgrel=1
pkgdesc="Interval break reminders, built for Omarchy"
arch=('x86_64' 'aarch64')
url="https://github.com/mauneven/pharos"
license=('MIT')
depends=('gtk4' 'glib2' 'cairo' 'pango' 'gcc-libs' 'glibc' 'hicolor-icon-theme')
optdepends=('libnotify: notifications outside Omarchy')
makedepends=('cargo')
options=('!lto')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
sha256sums=('32556a3aaf6172dd4c685f34d3f2bab0cadc2530f33f446fc6871f36c4c14a3b')

prepare() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

check() {
  cd "$pkgname-$pkgver"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=target
  cargo test --frozen --release
}

package() {
  cd "$pkgname-$pkgver"
  make DESTDIR="$pkgdir" PREFIX=/usr install
}
