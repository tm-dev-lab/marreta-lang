#!/bin/sh
# Marreta Lang installer.
#
# Downloads a published release binary for the host platform and installs it on
# the user's PATH. Usage:
#
#   curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/tm-dev-lab/marreta-lang/main/install.sh | sh -s -- v0.2.0
#
# Environment overrides:
#   MARRETA_VERSION      pin a release tag (same as the positional argument)
#   MARRETA_TARGET       force an asset name (e.g. marreta-macos-x86_64)
#   MARRETA_INSTALL_DIR  install directory (default: ~/.local/bin)
#
# Supported targets: Linux and macOS on x86_64 and arm64. Windows is supported
# through WSL, where this script resolves to the Linux build.

set -eu

REPO="tm-dev-lab/marreta-lang"

err() {
    printf 'marreta install: %s\n' "$1" >&2
    exit 1
}

info() {
    printf 'marreta install: %s\n' "$1" >&2
}

# ── Resolve the asset for this host (or honor an explicit override) ──────────
target="${MARRETA_TARGET:-}"
if [ -z "$target" ]; then
    os="$(uname -s)"
    arch="$(uname -m)"

    case "$os" in
        Linux) os_part="linux" ;;
        Darwin) os_part="macos" ;;
        *) err "unsupported OS '$os'. Linux and macOS are supported; on Windows, run this inside WSL. See the manual download instructions in the README." ;;
    esac

    case "$arch" in
        x86_64 | amd64) arch_part="x86_64" ;;
        arm64 | aarch64) arch_part="arm64" ;;
        *) err "unsupported architecture '$arch'. Supported: x86_64 and arm64. See the manual download instructions in the README." ;;
    esac

    target="marreta-${os_part}-${arch_part}"
fi

# ── Resolve the release URL (latest by default, or a pinned version) ─────────
version="${1:-${MARRETA_VERSION:-}}"
if [ -n "$version" ]; then
    url="https://github.com/${REPO}/releases/download/${version}/${target}"
    info "installing ${target} (${version})"
else
    # GitHub's stable redirect, so the default path needs no API call or jq.
    url="https://github.com/${REPO}/releases/latest/download/${target}"
    info "installing ${target} (latest)"
fi

# ── Download to a temporary file ─────────────────────────────────────────────
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT INT TERM

if command -v curl >/dev/null 2>&1; then
    curl -fL --proto '=https' --tlsv1.2 -o "$tmp" "$url" ||
        err "download failed: $url (does asset '$target' exist for this release?)"
elif command -v wget >/dev/null 2>&1; then
    wget -O "$tmp" "$url" ||
        err "download failed: $url (does asset '$target' exist for this release?)"
else
    err "need curl or wget to download the release."
fi

# ── Install onto PATH (no sudo, no profile edits) ────────────────────────────
install_dir="${MARRETA_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$install_dir"
chmod +x "$tmp"
mv "$tmp" "$install_dir/marreta"

# ── Verify by absolute path, so this never depends on PATH ───────────────────
info "installed to $install_dir/marreta"
"$install_dir/marreta" --version

# ── Hint if the install directory is not on PATH (we do not edit profiles) ───
case ":$PATH:" in
    *":$install_dir:"*) ;;
    *)
        info "note: $install_dir is not on your PATH."
        info "add this to your shell profile to run 'marreta' directly:"
        # SC2016: $PATH is meant to stay literal here; it is the line the user copies.
        # shellcheck disable=SC2016
        printf '\n  export PATH="%s:$PATH"\n\n' "$install_dir" >&2
        ;;
esac
