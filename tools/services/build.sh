#!/bin/sh
# Builds and fetches the network services slot ships for the H700 into OUT (vendor/services):
#
#   tailscaled         Tailscale, daemon and CLI in one small binary (the CLI runs with
#                      TS_BE_CLI=1), built from a pinned tag with only the features slot uses
#   wireguard-go       WireGuard in userspace, built from a pinned commit, fully static
#   syncthing          Syncthing's own static release build, pinned and checked
#   samba/             spruceOS's prebuilt Samba 3.6.23 for aarch64 glibc, from a pinned commit,
#                      with the libnsl.so.1 it needs and BaseOS lacks, from Ubuntu jammy's libc6
#   services.manifest  where each came from and its sha256
#
# Go is downloaded, pinned and checked, unless a new enough one is already on PATH. Everything
# is cross-compiled, so any Linux host with curl, tar, sha256sum, ar and zstd will do.
set -eu

OUT="${1:-vendor/services}"
GO_VERSION=1.27.2
GO_SHA_AMD64=ecbadb99091a3f46e31f5f934b068b1864eafa7995211b39eaddf76996045fe5
GO_SHA_ARM64=94f3e30b8e374bc285e7dadc11e0865726b9bc6e85b841ccceaabc0214c6b7c8
TAILSCALE_TAG=v1.104.1
WIREGUARD_GO_COMMIT=2631ce99a06f27120d581611cf125d68bc6aa565
SPRUCE_COMMIT=590ebbb2e2ccf066ced623e2405710c1483b2558
# The glibc BaseOS's own is harvested from, so its libnsl wants nothing that one lacks.
SYNCTHING_VERSION=v2.1.6
SYNCTHING_SHA=f054de02462d5d58bd126908f2f92b63aba74f162a5be85e50e96d656e7db47d
LIBC6_DEB=libc6_2.35-0ubuntu3.15_arm64.deb
LIBC6_SHA=dd88cc85dcfbac19c0c9b606676b7b1b87f851d75afa5c830fc5ba1293a30833
CACHE="${SERVICES_CACHE:-$HOME/.cache/unslot-services}"

mkdir -p "$OUT" "$CACHE"
OUT="$(cd "$OUT" && pwd)"
manifest="$OUT/services.manifest.new"
: > "$manifest"

go_ready() {
	command -v go >/dev/null 2>&1 || return 1
	have="$(go env GOVERSION 2>/dev/null | sed 's/^go//')"
	[ "$(printf '%s\n%s\n' "$GO_VERSION" "$have" | sort -V | head -n 1)" = "$GO_VERSION" ]
}

if ! go_ready; then
	case "$(uname -m)" in
	x86_64) arch=amd64 sha=$GO_SHA_AMD64 ;;
	aarch64 | arm64) arch=arm64 sha=$GO_SHA_ARM64 ;;
	*) echo "no Go for $(uname -m)" >&2; exit 1 ;;
	esac
	if [ ! -x "$CACHE/go-$GO_VERSION/bin/go" ]; then
		tgz="$CACHE/go$GO_VERSION.linux-$arch.tar.gz"
		curl -fsSL -o "$tgz" "https://dl.google.com/go/go$GO_VERSION.linux-$arch.tar.gz"
		echo "$sha  $tgz" | sha256sum -c - >/dev/null
		rm -rf "$CACHE/go-$GO_VERSION" "$CACHE/go"
		tar -xzf "$tgz" -C "$CACHE"
		mv "$CACHE/go" "$CACHE/go-$GO_VERSION"
	fi
	PATH="$CACHE/go-$GO_VERSION/bin:$PATH"
	export PATH
fi
export GOOS=linux GOARCH=arm64 CGO_ENABLED=0 GOFLAGS=-mod=mod GOTOOLCHAIN=local

note() {
	echo "$1 $(sha256sum "$2" | cut -d' ' -f1) $3" >> "$manifest"
}

# Tailscale.
src="$CACHE/tailscale-$TAILSCALE_TAG"
# build_dist.sh names the version from git, so this is a clone rather than an archive.
if [ ! -d "$src/.git" ]; then
	rm -rf "$src"
	git clone -q --depth 1 --branch "$TAILSCALE_TAG" https://github.com/tailscale/tailscale.git "$src"
fi
# What --extra-small keeps (routing through the kernel's TUN, NAT traversal) plus netstack, so
# userspace networking is there to fall back on should a device lack /dev/net/tun; and the CLI.
(cd "$src" &&
	TAGS="$(GOOS= GOARCH= go run ./cmd/featuretags --min --add=osrouter,nattraversal,netstack,ipnbus),ts_include_cli" 		./build_dist.sh --strip -o "$OUT/tailscaled" ./cmd/tailscaled)
note tailscaled "$OUT/tailscaled" "https://github.com/tailscale/tailscale/tree/$TAILSCALE_TAG"

# WireGuard.
src="$CACHE/wireguard-go-$WIREGUARD_GO_COMMIT"
if [ ! -d "$src" ]; then
	curl -fsSL "https://github.com/WireGuard/wireguard-go/archive/$WIREGUARD_GO_COMMIT.tar.gz" |
		tar -xz -C "$CACHE"
fi
(cd "$src" && go build -trimpath -ldflags='-s -w' -o "$OUT/wireguard-go" .)
note wireguard-go "$OUT/wireguard-go" "https://github.com/WireGuard/wireguard-go/tree/$WIREGUARD_GO_COMMIT"

# Syncthing, as it releases it.
url="https://github.com/syncthing/syncthing/releases/download/$SYNCTHING_VERSION/syncthing-linux-arm64-$SYNCTHING_VERSION.tar.gz"
tgz="$CACHE/syncthing-linux-arm64-$SYNCTHING_VERSION.tar.gz"
[ -f "$tgz" ] || curl -fsSL -o "$tgz" "$url"
echo "$SYNCTHING_SHA  $tgz" | sha256sum -c - >/dev/null
tar -xzf "$tgz" -C "$OUT" --strip-components=1 "syncthing-linux-arm64-$SYNCTHING_VERSION/syncthing"
note syncthing "$OUT/syncthing" "$url"

# Samba, prebuilt by spruceOS.
raw="https://raw.githubusercontent.com/spruceUI/spruceOS/$SPRUCE_COMMIT/spruce/bin64/Samba"
for f in bin/smbd bin/smbpasswd \
	lib/libnetapi.so.0 lib/libsmbclient.so.0 lib/libsmbsharemodes.so.0 \
	lib/libtalloc.so.2 lib/libtdb.so.1 lib/libtevent.so.0 lib/lowcase.dat lib/upcase.dat \
	lib/auth/script.so lib/charset/CP437.so lib/charset/CP850.so lib/idmap/autorid.so; do
	mkdir -p "$OUT/samba/$(dirname "$f")"
	curl -fsSL -o "$OUT/samba/$f" "$raw/$f"
	note "samba/$f" "$OUT/samba/$f" "$raw/$f"
done
chmod 755 "$OUT/samba/bin/smbd" "$OUT/samba/bin/smbpasswd"
# Launchpad keeps every published version, where the ports pool drops superseded ones.
# It answers 503 now and then, so the fetch tries again.
url="https://launchpad.net/ubuntu/+archive/primary/+files/$LIBC6_DEB"
deb="$CACHE/$LIBC6_DEB"
[ -f "$deb" ] || curl -fsSL --retry 5 --retry-delay 10 -o "$deb" "$url"
echo "$LIBC6_SHA  $deb" | sha256sum -c - >/dev/null
tmp="$(mktemp -d)"
(cd "$tmp" && ar x "$deb" data.tar.zst && zstd -dcq data.tar.zst |
	tar -x ./lib/aarch64-linux-gnu/libnsl.so.1)
cp "$tmp/lib/aarch64-linux-gnu/libnsl.so.1" "$OUT/samba/lib/libnsl.so.1"
rm -rf "$tmp"
note samba/lib/libnsl.so.1 "$OUT/samba/lib/libnsl.so.1" "$url"

for bin in "$OUT/tailscaled" "$OUT/wireguard-go" "$OUT/syncthing" "$OUT/samba/bin/smbd"; do
	file "$bin" | grep -q aarch64 || { echo "$bin is not aarch64" >&2; exit 1; }
done
mv "$manifest" "$OUT/services.manifest"
ls -la "$OUT" "$OUT/samba/bin"
