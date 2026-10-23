#!/bin/sh
# Luna installer (POSIX).
# Usage: curl -fsSL https://luna.tngklp.com/install | sh
#
# Downloads the latest luna release binary for the TUI from GitHub releases
# and installs it to ~/.local/bin (or /usr/local/bin with sudo fallback).
set -eu

REPO="tngklp/luna"
INSTALL_DIR="${LUNA_INSTALL_DIR:-$HOME/.local/bin}"

main() {
    echo "Installing Luna (TUI)..."

    # Detect platform.
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Linux) os_tag="unknown-linux-gnu" ;;
        Darwin) os_tag="apple-darwin" ;;
        *)
            echo "error: unsupported OS '$os'. On Windows use:" >&2
            echo "  irm https://luna.tngklp.com/install.ps1 | iex" >&2
            exit 1
            ;;
    esac
    case "$arch" in
        x86_64|amd64) arch_tag="x86_64" ;;
        aarch64|arm64) arch_tag="aarch64" ;;
        *) echo "error: unsupported architecture '$arch'" >&2; exit 1 ;;
    esac

    target="${arch_tag}-${os_tag}"
    asset="luna-${target}.tar.gz"
    url="https://github.com/${REPO}/releases/latest/download/${asset}"

    # Work in a temp dir.
    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT

    echo "Downloading ${url}..."
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$tmp/$asset"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$tmp/$asset" "$url"
    else
        echo "error: need curl or wget" >&2
        exit 1
    fi

    tar -xzf "$tmp/$asset" -C "$tmp"

    mkdir -p "$INSTALL_DIR"
    mv "$tmp/luna" "$INSTALL_DIR/luna"
    chmod +x "$INSTALL_DIR/luna"

    # PATH check.
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo
            echo "NOTE: $INSTALL_DIR is not in your PATH."
            shell_rc="$HOME/.profile"
            [ -n "${ZSH_VERSION:-}" ] && shell_rc="$HOME/.zshrc"
            [ -n "${BASH_VERSION:-}" ] && shell_rc="$HOME/.bashrc"
            echo "Add it with:  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> $shell_rc"
            ;;
    esac

    echo
    echo "Luna installed to $INSTALL_DIR/luna"
    echo "Run 'luna' to start. Configure providers in:"
    echo "  ~/.config/luna/Luna/config.toml"
}

main "$@"
