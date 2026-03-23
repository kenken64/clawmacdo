#!/usr/bin/env bash
#
# release.sh — Automated release workflow for clawmacdo
#
# Usage:
#   ./scripts/release.sh [patch|minor|major]
#
# Default: minor bump
#
# Steps:
#   1. cargo fmt --all
#   2. cargo clippy -- -D warnings
#   3. Bump version in all Cargo.toml and package.json files
#   4. Update CHANGELOG.md and README.md version references
#   5. Sync README to npm/clawmacdo/README.md
#   6. cargo build --release
#   7. Git commit, tag, push
#   8. Create GitHub release (triggers release + npm-publish workflows)

set -euo pipefail

BUMP_TYPE="${1:-minor}"

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep -m1 '^version' crates/clawmacdo-cli/Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

case "$BUMP_TYPE" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) echo "Usage: $0 [patch|minor|major]"; exit 1 ;;
esac

NEW_VERSION="$MAJOR.$MINOR.$PATCH"
echo "Bumping: $CURRENT_VERSION -> $NEW_VERSION ($BUMP_TYPE)"

echo "==> Formatting..."
cargo fmt --all

echo "==> Linting..."
cargo clippy -- -D warnings

echo "==> Bumping versions to $NEW_VERSION..."
# Update all Cargo.toml files
find crates -name Cargo.toml -exec sed -i '' "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" {} +

# Update package.json files
for f in npm/clawmacdo/package.json npm/@clawmacdo/darwin-arm64/package.json npm/@clawmacdo/linux-x64/package.json npm/@clawmacdo/win32-x64/package.json; do
  if [ -f "$f" ]; then
    sed -i '' "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/g" "$f"
    sed -i '' "s/\"@clawmacdo\/darwin-arm64\": \"$CURRENT_VERSION\"/\"@clawmacdo\/darwin-arm64\": \"$NEW_VERSION\"/g" "$f"
    sed -i '' "s/\"@clawmacdo\/linux-x64\": \"$CURRENT_VERSION\"/\"@clawmacdo\/linux-x64\": \"$NEW_VERSION\"/g" "$f"
    sed -i '' "s/\"@clawmacdo\/win32-x64\": \"$CURRENT_VERSION\"/\"@clawmacdo\/win32-x64\": \"$NEW_VERSION\"/g" "$f"
  fi
done

echo "==> Updating CHANGELOG and README..."
sed -i '' "s/## v$CURRENT_VERSION/## v$NEW_VERSION/" CHANGELOG.md
sed -i '' "s/What's New in v$CURRENT_VERSION/What's New in v$NEW_VERSION/" README.md
sed -i '' "s/Current version:.*/Current version:** $NEW_VERSION/" README.md

echo "==> Syncing README to npm..."
cp README.md npm/clawmacdo/README.md

echo "==> Building release..."
cargo build --release

echo "==> Committing and tagging..."
git add -A
git commit -m "chore: release v$NEW_VERSION"
git tag "v$NEW_VERSION"
git push origin main
git push origin "v$NEW_VERSION"

echo "==> Creating GitHub release..."
gh release create "v$NEW_VERSION" \
  --title "v$NEW_VERSION" \
  --notes "Release v$NEW_VERSION. See [CHANGELOG.md](https://github.com/kenken64/clawmacdo/blob/main/CHANGELOG.md) for details."

echo ""
echo "=== Release v$NEW_VERSION published ==="
echo "  Release: https://github.com/kenken64/clawmacdo/releases/tag/v$NEW_VERSION"
echo "  npm-publish and release workflows triggered."
