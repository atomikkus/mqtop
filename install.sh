#!/bin/sh
set -eu

REPO="${MQTOP_REPO:-atomikkus/mqtop}"
BIN_DIR="${MQTOP_BIN_DIR:-${HOME}/.local/bin}"
INSTALL_AGENT="${MQTOP_INSTALL_AGENT:-0}"
TARGET="${MQTOP_TARGET:-}"
VERSION="${MQTOP_VERSION:-}"
RELEASE_ROOT="${MQTOP_RELEASE_ROOT:-https://github.com/${REPO}/releases}"
USE_GH="${MQTOP_USE_GH:-auto}"

say() {
    printf '%s\n' "$*"
}

fail() {
    say "mqtop installer: $*" >&2
    exit 1
}

command -v tar >/dev/null 2>&1 || fail "tar is required"
command -v install >/dev/null 2>&1 || fail "install is required"

[ "$(uname -s)" = "Linux" ] || fail "only Linux is currently supported"

if [ "$USE_GH" = "auto" ]; then
    if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
        USE_GH=1
    else
        USE_GH=0
    fi
fi

if [ "$USE_GH" = "1" ]; then
    command -v gh >/dev/null 2>&1 || fail "GitHub CLI is required for private releases"
else
    command -v curl >/dev/null 2>&1 || fail "curl is required"
fi

if [ -z "$TARGET" ]; then
    case "$(uname -m)" in
        x86_64|amd64) TARGET="x86_64-unknown-linux-musl" ;;
        aarch64|arm64) TARGET="aarch64-unknown-linux-musl" ;;
        *) fail "unsupported architecture: $(uname -m)" ;;
    esac
fi

if [ -z "$VERSION" ]; then
    if [ "$USE_GH" = "1" ]; then
        command -v gh >/dev/null 2>&1 ||
            fail "MQTOP_USE_GH=1 requires the GitHub CLI"
        VERSION="$(gh api "repos/${REPO}/releases/latest" --jq .tag_name)" ||
            fail "could not resolve the latest private release"
    else
        latest_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "${RELEASE_ROOT}/latest")" ||
            fail "could not resolve the latest release"
        VERSION="${latest_url##*/}"
    fi
fi

case "$VERSION" in
    v*) ;;
    *) VERSION="v${VERSION}" ;;
esac

asset="mqtop-${VERSION}-${TARGET}.tar.gz"
base="${RELEASE_ROOT}/download/${VERSION}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

say "Downloading mqtop ${VERSION} for ${TARGET}..."
if [ "$USE_GH" = "1" ]; then
    gh release download "$VERSION" \
        --repo "$REPO" \
        --pattern "$asset" \
        --pattern "${asset}.sha256" \
        --dir "$tmp" \
        --clobber ||
        fail "could not download private release assets"
else
    curl -fsSL "${base}/${asset}" -o "${tmp}/${asset}" ||
        fail "release asset not found: ${base}/${asset}"
    curl -fsSL "${base}/${asset}.sha256" -o "${tmp}/${asset}.sha256" ||
        fail "checksum not found: ${base}/${asset}.sha256"
fi

(
    cd "$tmp"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -c "${asset}.sha256"
    elif command -v shasum >/dev/null 2>&1; then
        expected="$(cut -d ' ' -f 1 "${asset}.sha256")"
        actual="$(shasum -a 256 "$asset" | cut -d ' ' -f 1)"
        [ "$expected" = "$actual" ] || fail "checksum verification failed"
    else
        fail "sha256sum or shasum is required"
    fi
)

tar -xzf "${tmp}/${asset}" -C "$tmp"
mkdir -p "$BIN_DIR"
install -m 0755 "${tmp}/mqtop-rs" "${BIN_DIR}/mqtop-rs"

if [ "$INSTALL_AGENT" = "1" ]; then
    install -m 0755 "${tmp}/mqtop-agent" "${BIN_DIR}/mqtop-agent"
fi

say "Installed mqtop-rs to ${BIN_DIR}/mqtop-rs"
if [ "$INSTALL_AGENT" = "1" ]; then
    say "Installed mqtop-agent to ${BIN_DIR}/mqtop-agent"
else
    say "Fleet agent not installed (set MQTOP_INSTALL_AGENT=1 to include it)."
fi

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) say "Add ${BIN_DIR} to PATH, then restart your shell." ;;
esac

"${BIN_DIR}/mqtop-rs" --version
