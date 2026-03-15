#!/usr/bin/env sh
set -e

REPO="rustkit-ai/memo"
BIN="memo"

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux*)  OS_NAME="linux" ;;
    Darwin*) OS_NAME="macos" ;;
    *)
        echo "Unsupported OS: $OS"
        exit 1
        ;;
esac

# Detect architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)          ARCH_NAME="x86_64" ;;
    aarch64|arm64)   ARCH_NAME="aarch64" ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

# Map to Rust target triple
if [ "$OS_NAME" = "linux" ]; then
    TARGET="${ARCH_NAME}-unknown-linux-gnu"
elif [ "$OS_NAME" = "macos" ]; then
    TARGET="${ARCH_NAME}-apple-darwin"
fi

ASSET="memo-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"

echo "Downloading memo for ${TARGET}..."

# Determine install location
if [ -w /usr/local/bin ]; then
    INSTALL_DIR="/usr/local/bin"
elif [ -w "$HOME/.local/bin" ]; then
    INSTALL_DIR="$HOME/.local/bin"
else
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
fi

# Download and extract
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$URL" -o "$TMP_DIR/$ASSET"
elif command -v wget >/dev/null 2>&1; then
    wget -q "$URL" -O "$TMP_DIR/$ASSET"
else
    echo "Error: curl or wget is required"
    exit 1
fi

tar xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"
install -m 755 "$TMP_DIR/$BIN" "$INSTALL_DIR/$BIN"

echo "memo installed to $INSTALL_DIR/memo"

# Remind user to add to PATH if using ~/.local/bin
case ":${PATH}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "Note: add the following to your shell profile to use memo:"
        echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac
