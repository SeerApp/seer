#!/usr/bin/env sh
# Pack seer + the pinned libz3 into seer-$OS-$ARCH.tar.gz.
# Reads z3-pin, fetches that Microsoft zip, links against it (not Homebrew).
set -eu

err() {
    echo "[ERROR] $1" >&2
    exit 1
}

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PIN="$ROOT/z3-pin"
OUT="$ROOT/dist"
TARGET=""

while [ $# -gt 0 ]; do
    case "$1" in
        --out)
            OUT=$2
            shift 2
            ;;
        --target)
            TARGET=$2
            shift 2
            ;;
        *)
            err "unknown argument: $1"
            ;;
    esac
done

[ -f "$PIN" ] || err "missing $PIN"

detect_target() {
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)
    case "$os" in
        mingw* | msys* | cygwin*) os=windows ;;
    esac
    case "$os:$arch" in
        darwin:arm64 | darwin:aarch64) echo aarch64-apple-darwin ;;
        darwin:x86_64) echo x86_64-apple-darwin ;;
        linux:x86_64 | linux:amd64) echo x86_64-unknown-linux-gnu ;;
        linux:aarch64 | linux:arm64) echo aarch64-unknown-linux-gnu ;;
        windows:x86_64 | windows:amd64) echo x86_64-pc-windows-msvc ;;
        windows:aarch64 | windows:arm64) echo aarch64-pc-windows-msvc ;;
        *) err "unsupported uname $(uname -s) $(uname -m); pass --target" ;;
    esac
}

short_name() {
    case "$1" in
        aarch64-apple-darwin) echo darwin-arm64 ;;
        x86_64-apple-darwin) echo darwin-amd64 ;;
        x86_64-unknown-linux-gnu) echo linux-amd64 ;;
        aarch64-unknown-linux-gnu) echo linux-arm64 ;;
        x86_64-pc-windows-msvc) echo windows-amd64 ;;
        aarch64-pc-windows-msvc) echo windows-arm64 ;;
        *) err "no archive name for target $1" ;;
    esac
}

pin_line() {
    target=$1
    awk -v t="$target" '
        $1 == t { print $2, $3; found=1 }
        END { if (!found) exit 1 }
    ' "$PIN" || err "no z3-pin row for $target"
}

sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

[ -n "$TARGET" ] || TARGET=$(detect_target)
SHORT=$(short_name "$TARGET")
VERSION=$(awk -F= '/^VERSION=/ { print $2; exit }' "$PIN")
set -- $(pin_line "$TARGET")
ASSET=$1
EXPECT=$2
URL="https://github.com/Z3Prover/z3/releases/download/z3-${VERSION}/${ASSET}"

mkdir -p "$OUT"
CACHE="${Z3_CACHE:-$OUT/.z3}"
mkdir -p "$CACHE"
ZIP="$CACHE/$ASSET"
if [ ! -f "$ZIP" ]; then
    echo "Downloading $URL"
    curl -fsSL -o "$ZIP" "$URL"
fi
GOT=$(sha256_file "$ZIP")
[ "$GOT" = "$EXPECT" ] || err "z3 zip sha256 mismatch: got $GOT want $EXPECT"

Z3ROOT="$CACHE/${ASSET%.zip}"
if [ ! -f "$Z3ROOT/include/z3.h" ]; then
    rm -rf "$Z3ROOT"
    mkdir -p "$CACHE"
    unzip -q "$ZIP" -d "$CACHE"
fi
[ -f "$Z3ROOT/include/z3.h" ] || err "unzipped Z3 missing include/z3.h"
Z3BIN="$Z3ROOT/bin"
[ -d "$Z3BIN" ] || err "unzipped Z3 missing bin/"

# The zip ships libz3.a next to the dylib. -lz3 would static-link that .a.
if [ -f "$Z3BIN/libz3.a" ]; then
    mv "$Z3BIN/libz3.a" "$Z3BIN/libz3.a.bak"
fi

PC="$CACHE/pkgconfig"
mkdir -p "$PC"
cat >"$PC/z3.pc" <<EOF
prefix=$Z3ROOT
libdir=$Z3BIN
includedir=$Z3ROOT/include
Name: z3
Description: pinned Z3
Version: $VERSION
Libs: -L\${libdir} -lz3
Cflags: -I\${includedir}
EOF

export PKG_CONFIG_LIBDIR="$PC"
export PKG_CONFIG_PATH="$PC"
export Z3_SYS_Z3_HEADER="$Z3ROOT/include/z3.h"
export Z3_LIBRARY_PATH_OVERRIDE="$Z3BIN"

case "$TARGET" in
    *windows*)
        if [ -f "$Z3BIN/libz3.lib" ] && [ ! -f "$Z3BIN/z3.lib" ]; then
            cp "$Z3BIN/libz3.lib" "$Z3BIN/z3.lib"
        fi
        if command -v cygpath >/dev/null 2>&1; then
            Z3BIN_NATIVE=$(cygpath -w "$Z3BIN")
            Z3_SYS_Z3_HEADER=$(cygpath -w "$Z3_SYS_Z3_HEADER")
        else
            Z3BIN_NATIVE=$Z3BIN
        fi
        export Z3_SYS_Z3_HEADER
        export Z3_LIBRARY_PATH_OVERRIDE="$Z3BIN_NATIVE"
        # Search path only. -l z3 in RUSTFLAGS also hits proc-macro build scripts.
        export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-L native=$Z3BIN_NATIVE"
        export LIB="${Z3BIN_NATIVE}${LIB:+;$LIB}"
        ;;
esac

HOST=$(rustc -vV | awk '/^host:/ { print $2 }')
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
echo "Building seer for $TARGET against $ASSET"
cd "$ROOT"
if [ "$TARGET" = "$HOST" ]; then
    cargo build -p seer --release
    DEST="$TARGET_DIR/release"
else
    cargo build -p seer --release --target "$TARGET"
    DEST="$TARGET_DIR/$TARGET/release"
fi

BIN=""
for cand in "$DEST/seer.exe" "$DEST/seer"; do
    if [ -f "$cand" ]; then
        BIN=$cand
        break
    fi
done
[ -n "$BIN" ] || err "cargo did not produce seer in $DEST"
case "$BIN" in
    *.exe) STAGE_BIN=seer.exe ;;
    *) STAGE_BIN=seer ;;
esac

STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
cp "$BIN" "$STAGE/$STAGE_BIN"
chmod +x "$STAGE/$STAGE_BIN" 2>/dev/null || true

copy_runtime_z3() {
    case "$TARGET" in
        *apple*)
            cp "$Z3BIN/libz3.dylib" "$STAGE/libz3.dylib"
            ;;
        *windows*)
            cp "$Z3BIN/libz3.dll" "$STAGE/libz3.dll"
            ;;
        *)
            cp "$Z3BIN/libz3.so" "$STAGE/libz3.so"
            ;;
    esac
}
copy_runtime_z3

rewrite_rpath() {
    case "$TARGET" in
        *apple*)
            install_name_tool -id @loader_path/libz3.dylib "$STAGE/libz3.dylib"
            otool -L "$STAGE/$STAGE_BIN" | awk '/libz3/ { print $1 }' | while read -r dep; do
                [ -n "$dep" ] || continue
                install_name_tool -change "$dep" @loader_path/libz3.dylib "$STAGE/$STAGE_BIN"
            done
            install_name_tool -add_rpath @loader_path "$STAGE/$STAGE_BIN" 2>/dev/null || true
            codesign --force -s - "$STAGE/$STAGE_BIN" 2>/dev/null || true
            codesign --force -s - "$STAGE/libz3.dylib" 2>/dev/null || true
            ;;
        *linux* | *unknown-linux*)
            if command -v patchelf >/dev/null 2>&1; then
                patchelf --set-rpath '$ORIGIN' "$STAGE/$STAGE_BIN"
            else
                err "need patchelf to set \$ORIGIN rpath"
            fi
            ;;
    esac
}
rewrite_rpath

ARCHIVE="$OUT/seer-${SHORT}.tar.gz"
if [ "$STAGE_BIN" = seer.exe ]; then
    tar -C "$STAGE" -czf "$ARCHIVE" seer.exe libz3.dll
else
    case "$TARGET" in
        *apple*) tar -C "$STAGE" -czf "$ARCHIVE" seer libz3.dylib ;;
        *) tar -C "$STAGE" -czf "$ARCHIVE" seer libz3.so ;;
    esac
fi

SUM=$(sha256_file "$ARCHIVE")
echo "$SUM" >"$ARCHIVE.sha256"
echo "Wrote $ARCHIVE"
echo "$SUM  seer-${SHORT}.tar.gz"
