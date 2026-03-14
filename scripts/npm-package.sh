#!/usr/bin/env bash
# Build and package clawmacdo binaries into npm platform packages.
#
# Usage:
#   ./scripts/npm-package.sh            # Build all platforms (requires cross-compilation)
#   ./scripts/npm-package.sh --local    # Build only for current platform
#
# After running, the npm/ directory will contain ready-to-publish packages.
# Publish with: ./scripts/npm-publish.sh

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NPM_DIR="$ROOT/npm"

# Read version from Cargo.toml workspace
VERSION=$(grep -m1 'version' "$ROOT/crates/clawmacdo-cli/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
echo "Building clawmacdo v${VERSION} for npm distribution"

# Resolve npm platform dir and binary name from a rust target
resolve_target() {
  local target="$1"
  case "$target" in
    x86_64-unknown-linux-gnu)  PLATFORM_DIR="linux-x64";    BINARY_NAME="clawmacdo" ;;
    aarch64-apple-darwin)      PLATFORM_DIR="darwin-arm64";  BINARY_NAME="clawmacdo" ;;
    x86_64-pc-windows-gnu)     PLATFORM_DIR="win32-x64";    BINARY_NAME="clawmacdo.exe" ;;
    *) echo "Error: unknown target $target"; exit 1 ;;
  esac
}

build_target() {
  local target="$1"
  resolve_target "$target"
  local dest_dir="$NPM_DIR/@clawmacdo/$PLATFORM_DIR/bin"

  echo "→ Building for $target ($PLATFORM_DIR)..."

  cargo build --release --target "$target"

  mkdir -p "$dest_dir"
  cp "target/$target/release/$BINARY_NAME" "$dest_dir/$BINARY_NAME"
  chmod +x "$dest_dir/$BINARY_NAME"

  echo "  ✓ Copied to $dest_dir/$BINARY_NAME"
}

# Update versions in all package.json files
update_versions() {
  echo "→ Updating package versions to $VERSION..."

  for pkg_json in \
    "$NPM_DIR/clawmacdo/package.json" \
    "$NPM_DIR/@clawmacdo/darwin-arm64/package.json" \
    "$NPM_DIR/@clawmacdo/linux-x64/package.json" \
    "$NPM_DIR/@clawmacdo/win32-x64/package.json"; do
    if [ -f "$pkg_json" ]; then
      sed -i.bak "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" "$pkg_json"
      sed -i.bak "s/\"@clawmacdo\/\(.*\)\": \".*\"/\"@clawmacdo\/\1\": \"$VERSION\"/" "$pkg_json"
      rm -f "${pkg_json}.bak"
    fi
  done

  echo "  ✓ All package versions set to $VERSION"
}

update_versions

ALL_TARGETS="x86_64-unknown-linux-gnu aarch64-apple-darwin x86_64-pc-windows-gnu"

if [ "${1:-}" = "--local" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)  build_target "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "Error: darwin-x64 not configured in npm packages"; exit 1 ;;
    Linux-x86_64)  build_target "x86_64-unknown-linux-gnu" ;;
    *)             echo "Error: unsupported platform $(uname -s)-$(uname -m)"; exit 1 ;;
  esac
else
  for target in $ALL_TARGETS; do
    build_target "$target"
  done
fi

echo ""
echo "Done! Packages ready in $NPM_DIR"
echo "To publish, run: ./scripts/npm-publish.sh"
