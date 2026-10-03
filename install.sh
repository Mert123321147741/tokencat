#!/bin/sh
# Install the latest tokencat release binary.
#
#   curl -fsSL https://raw.githubusercontent.com/OWNER/tokencat/main/install.sh | sh
#
# Environment:
#   TOKENCAT_REPO     GitHub repository to download from (default: OWNER/tokencat)
#   TOKENCAT_VERSION  release tag such as v0.1.0 (default: latest)
#   TOKENCAT_BIN_DIR  install directory (default: ~/.local/bin)
set -eu

repo="${TOKENCAT_REPO:-OWNER/tokencat}"
version="${TOKENCAT_VERSION:-latest}"
bin_dir="${TOKENCAT_BIN_DIR:-$HOME/.local/bin}"

fail() {
    echo "tokencat install: $*" >&2
    exit 1
}

case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *) fail "unsupported OS $(uname -s); on Windows download the .zip from https://github.com/$repo/releases" ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "unsupported architecture $(uname -m)" ;;
esac

asset="tokencat-$arch-$os.tar.gz"
if [ "$version" = latest ]; then
    url="https://github.com/$repo/releases/latest/download/$asset"
else
    url="https://github.com/$repo/releases/download/$version/$asset"
fi

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO "$2" "$1"; }
else
    fail "curl or wget is required"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $url"
fetch "$url" "$tmp/$asset" || fail "download failed"
if fetch "$url.sha256" "$tmp/$asset.sha256" 2>/dev/null; then
    expected="$(cut -d' ' -f1 <"$tmp/$asset.sha256")"
    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$tmp/$asset" | cut -d' ' -f1)"
    else
        actual="$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"
    fi
    [ "$expected" = "$actual" ] || fail "checksum mismatch for $asset"
fi

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$bin_dir"
install -m 755 "$tmp/tokencat" "$bin_dir/tokencat"
echo "Installed $("$bin_dir/tokencat" --version) to $bin_dir/tokencat"

case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir is not on your PATH." ;;
esac
