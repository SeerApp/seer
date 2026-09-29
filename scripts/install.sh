#!/usr/bin/env sh
set -e
# Install seer + sibling libz3 from a GitHub release tarball (or --archive).
err() {
    echo "[ERROR] $1" >&2
    exit 1
}
need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "need '$1' (command not found)"
    fi
}
TMPDIR=""
trap 'if [ -n "$TMPDIR" ] && [ -d "$TMPDIR" ]; then rm -rf "$TMPDIR"; fi' EXIT

REPO="SeerApp/seer"
PREFIX=""
ARCHIVE=""

while [ $# -gt 0 ]; do
    case "$1" in
        --prefix)
            PREFIX=$2
            shift 2
            ;;
        --archive)
            ARCHIVE=$2
            shift 2
            ;;
        -h | --help)
            echo "usage: install.sh [--prefix DIR] [--archive PATH]"
            exit 0
            ;;
        *)
            err "unknown argument: $1"
            ;;
    esac
done

detect_platform() {
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)
    case $ARCH in
        x86_64 | amd64) ARCH=amd64 ;;
        aarch64 | arm64) ARCH=arm64 ;;
        *) err "Unsupported architecture: $ARCH" ;;
    esac
    case $OS in
        linux | darwin) ;;
        msys* | mingw* | cygwin*) OS=windows ;;
        *) err "Unsupported OS: $OS" ;;
    esac
    echo "$OS" "$ARCH"
}

get_latest_release() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | grep '"tag_name"' | head -1 | sed -E 's/.*: "([^"]+)".*/\1/'
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

choose_install_dir() {
    if [ -n "$PREFIX" ]; then
        mkdir -p "$PREFIX"
        echo "$PREFIX"
        return
    fi
    if [ -w "$HOME/.local/bin" ] 2>/dev/null || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
        if [ -w "$HOME/.local/bin" ]; then
            echo "$HOME/.local/bin"
            return
        fi
    fi
    if [ -w "/usr/local/bin" ]; then
        echo "/usr/local/bin"
        return
    fi
    mkdir -p "$HOME/.local/bin"
    echo "$HOME/.local/bin"
}

add_path_if_needed() {
    TARGET_DIR="$1"
    case "$SHELL" in
        */zsh) SHELL_RC="$HOME/.zshrc" ;;
        */bash) SHELL_RC="$HOME/.bashrc" ;;
        */fish) SHELL_RC="$HOME/.config/fish/config.fish" ;;
        *) SHELL_RC="$HOME/.profile" ;;
    esac
    mkdir -p "$(dirname "$SHELL_RC")"
    touch "$SHELL_RC"
    if ! grep -q "$TARGET_DIR" "$SHELL_RC"; then
        echo "export PATH=\"$TARGET_DIR:\$PATH\"" >>"$SHELL_RC"
        echo "[INFO] Added $TARGET_DIR to PATH in $SHELL_RC"
    fi
}

install_from_archive() {
    archive=$1
    OS=$2
    EXTRACT=$(mktemp -d)
    tar -xzf "$archive" -C "$EXTRACT"
    if [ "$OS" = windows ]; then
        BIN_NAME=seer.exe
    else
        BIN_NAME=seer
    fi
    if [ ! -f "$EXTRACT/$BIN_NAME" ]; then
        err "$BIN_NAME not found in archive."
    fi
    found_lib=0
    for lib in "$EXTRACT"/libz3.dylib "$EXTRACT"/libz3.so "$EXTRACT"/libz3.dll; do
        if [ -f "$lib" ]; then
            found_lib=1
            break
        fi
    done
    [ "$found_lib" = 1 ] || err "libz3 not found in archive."
    INSTALL_DIR=$(choose_install_dir)
    mv "$EXTRACT/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
    chmod +x "$INSTALL_DIR/$BIN_NAME" 2>/dev/null || true
    for lib in "$EXTRACT"/libz3.dylib "$EXTRACT"/libz3.so "$EXTRACT"/libz3.dll; do
        if [ -f "$lib" ]; then
            mv "$lib" "$INSTALL_DIR/"
        fi
    done
    rm -rf "$EXTRACT"
    echo "Installed to $INSTALL_DIR/$BIN_NAME"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *)
            echo "[INFO] $INSTALL_DIR is not in your PATH."
            if [ -z "$PREFIX" ] && [ "$INSTALL_DIR" = "$HOME/.local/bin" ] && [ "$(id -u)" != "0" ]; then
                add_path_if_needed "$INSTALL_DIR"
                echo "[INFO] Restart your terminal or run: source \"$HOME/.${SHELL##*/}rc\""
            else
                echo "[INFO] Add $INSTALL_DIR to your PATH to use 'seer'."
                echo "export PATH=\"$INSTALL_DIR:\$PATH\""
            fi
            ;;
    esac
}

main() {
    for cmd in tar mktemp chmod mv grep uname; do
        need_cmd "$cmd"
    done
    platform=$(detect_platform)
    OS=$(echo "$platform" | cut -d' ' -f1)
    ARCH=$(echo "$platform" | cut -d' ' -f2)
    FILENAME="seer-$OS-$ARCH.tar.gz"
    if [ -n "$ARCHIVE" ]; then
        [ -f "$ARCHIVE" ] || err "archive not found: $ARCHIVE"
        if [ -f "$ARCHIVE.sha256" ]; then
            expect=$(tr -d ' \n' <"$ARCHIVE.sha256" | awk '{print $1}')
            got=$(sha256_file "$ARCHIVE")
            [ "$got" = "$expect" ] || err "checksum mismatch for $ARCHIVE"
        fi
        install_from_archive "$ARCHIVE" "$OS"
        return
    fi
    need_cmd curl
    TAG=$(get_latest_release)
    [ -n "$TAG" ] || err "could not read latest GitHub release tag"
    echo "Detected OS: $OS, ARCH: $ARCH, Latest version: $TAG"
    URL="https://github.com/${REPO}/releases/download/${TAG}/${FILENAME}"
    TMPDIR=$(mktemp -d)
    echo "Downloading $URL ..."
    curl -fsSL "$URL" -o "$TMPDIR/$FILENAME" || err "Download failed. Check if this platform is supported."
    SUM_URL="$URL.sha256"
    if curl -fsSL "$SUM_URL" -o "$TMPDIR/$FILENAME.sha256"; then
        expect=$(tr -d ' \n' <"$TMPDIR/$FILENAME.sha256" | awk '{print $1}')
        got=$(sha256_file "$TMPDIR/$FILENAME")
        [ "$got" = "$expect" ] || err "checksum mismatch for $FILENAME"
    fi
    install_from_archive "$TMPDIR/$FILENAME" "$OS"
}

main "$@"
