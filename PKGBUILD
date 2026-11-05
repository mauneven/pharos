# Maintainer: mauneven <mauneven@gmail.com>
pkgname=pharos
pkgver=0.3.1
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
sha256sums=('8c54f22be49cd66dd0cb34c7fc68fffffc75497ee0838c6a5415b0e88038d141')

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
