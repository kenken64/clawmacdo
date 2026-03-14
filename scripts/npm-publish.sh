#!/usr/bin/env bash
# Publish clawmacdo npm packages to the registry.
#
# Prerequisites:
#   1. Run ./scripts/npm-package.sh first (or let CI do it)
#   2. npm login (or set NPM_TOKEN env var)
#   3. Ensure @clawmacdo org exists on npm: https://www.npmjs.com/org/clawmacdo
#
# Usage:
#   ./scripts/npm-publish.sh              # Publish to npm (production)
#   ./scripts/npm-publish.sh --dry-run    # Dry run (validate without publishing)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NPM_DIR="$ROOT/npm"
DRY_RUN=""

if [ "${1:-}" = "--dry-run" ]; then
  DRY_RUN="--dry-run"
  echo "=== DRY RUN MODE ==="
fi

# Publish platform packages first (they must exist before the root package)
PLATFORM_PACKAGES=(
  "@clawmacdo/darwin-arm64"
  "@clawmacdo/linux-x64"
  "@clawmacdo/win32-x64"
)

for pkg in "${PLATFORM_PACKAGES[@]}"; do
  pkg_dir="$NPM_DIR/$pkg"

  # Verify binary exists
  bin_dir="$pkg_dir/bin"
  if [ ! -d "$bin_dir" ] || [ -z "$(ls -A "$bin_dir" 2>/dev/null)" ]; then
    echo "⚠ Skipping $pkg (no binary found in $bin_dir)"
    continue
  fi

  echo "→ Publishing $pkg..."
  (cd "$pkg_dir" && npm publish --access public $DRY_RUN)
  echo "  ✓ Published $pkg"
done

# Copy repo README into root npm package so it shows on npmjs.com
cp "$ROOT/README.md" "$NPM_DIR/clawmacdo/README.md"

# Publish root package
echo "→ Publishing clawmacdo..."
(cd "$NPM_DIR/clawmacdo" && npm publish --access public $DRY_RUN)
echo "  ✓ Published clawmacdo"

echo ""
echo "Done! All packages published."
echo "Install with: npm install -g clawmacdo"
