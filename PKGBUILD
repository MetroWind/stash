pkgname=stash
pkgver=0.1.0
pkgrel=1
pkgdesc='A naively simple read-it-later service'
arch=(x86_64 i686 armv6h armv7h)
url='https://github.com/MetroWind/stash'
license=(WTFPL)
makedepends=(cargo sqlite rustup git)
depends=(sqlite)
source=("git+${url}.git")
sha256sums=('SKIP')

prepare() {
    rustup install --profile minimal nightly
}

build() {
	export CARGO_TARGET_DIR=target
	cargo build --release
}

package() {
	cd "$_archive"
	install -Dm0755 -t "$pkgdir/usr/bin/" "target/release/${pkgname}"
    install -D -t "$pkgdir/var/lib/${pkgname}/" templates
    install -D -t "$pkgdir/var/lib/${pkgname}/" static
    install -D stash-example.toml /etc/stash.toml
}
