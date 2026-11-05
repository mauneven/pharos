# Maintainer: mauneven <mauneven@gmail.com>
pkgname=beacon
pkgver=0.1.0
pkgrel=1
pkgdesc="Interval break reminders, built for Omarchy"
arch=('x86_64' 'aarch64')
url="https://github.com/mauneven/beacon"
license=('MIT')
depends=('gtk4' 'libnotify')
optdepends=('omarchy: themed notifications, live theme and language')
makedepends=('cargo' 'git')
source=("$pkgname::git+$url.git#tag=v$pkgver")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname"
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname"
  export RUSTUP_TOOLCHAIN=stable CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

check() {
  cd "$pkgname"
  cargo test --frozen
}

package() {
  cd "$pkgname"
  make DESTDIR="$pkgdir" PREFIX=/usr install
}
