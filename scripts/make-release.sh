#!/usr/bin/env bash
set -e

echo "Reel Release Automation"
echo "======================="
echo ""

# Check for clean working directory
if [ -n "$(git status --porcelain)" ]; then
  echo "Error: Working directory is not clean. Please commit or stash changes."
  exit 1
fi

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | cut -d'"' -f2)
echo "Current version: $CURRENT_VERSION"

# Parse version components
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT_VERSION"

# Determine version bump type (default: patch)
# "hit" keeps the upstream version and tags the next fork release on top of it:
# v<version>-hit.1, v<version>-hit.2, ...
BUMP_TYPE="${1:-patch}"

case "$BUMP_TYPE" in
  major)
    NEW_MAJOR=$((MAJOR + 1))
    NEW_VERSION="$NEW_MAJOR.0.0"
    ;;
  minor)
    NEW_MINOR=$((MINOR + 1))
    NEW_VERSION="$MAJOR.$NEW_MINOR.0"
    ;;
  patch)
    NEW_PATCH=$((PATCH + 1))
    NEW_VERSION="$MAJOR.$MINOR.$NEW_PATCH"
    ;;
  hit)
    NEW_VERSION="$CURRENT_VERSION"
    git fetch --tags origin
    LAST_HIT=$(git tag -l "v$CURRENT_VERSION-hit.*" | sed "s/^v$CURRENT_VERSION-hit\.//" | sort -n | tail -1)
    NEW_TAG="v$CURRENT_VERSION-hit.$((${LAST_HIT:-0} + 1))"
    ;;
  *)
    echo "Error: Invalid bump type. Use 'major', 'minor', 'patch' or 'hit'."
    exit 1
    ;;
esac

NEW_TAG="${NEW_TAG:-v$NEW_VERSION}"
echo "New version: $NEW_VERSION"
echo "New tag: $NEW_TAG"
echo ""

if [ "$NEW_VERSION" != "$CURRENT_VERSION" ]; then
  # Update version in Cargo.toml
  echo "Updating Cargo.toml..."
  sed -i.bak "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml
  rm Cargo.toml.bak

  # Update Cargo.lock
  echo "Updating Cargo.lock..."
  cargo update --package reel
fi

# Run tests to ensure everything still works
echo "Running tests..."
cargo test

# Build to ensure compilation
echo "Building project..."
cargo build --release

if [ "$NEW_VERSION" != "$CURRENT_VERSION" ]; then
  # Commit version bump
  echo "Creating release commit..."
  git add Cargo.toml Cargo.lock
  git commit -m "chore: release $NEW_TAG"
fi

# Generate release notes with AI
echo ""
echo "Generating release notes with AI..."
echo ""

# Generate notes and save to temp file.
# Capture stdout only: the generator writes its notes there and its progress and
# diagnostics to stderr. Merging the two put lines like "Trying Claude CLI..." at the
# top of the tag annotation, which is published verbatim as the release notes. Leaving
# stderr on the terminal also makes the progress visible while the generator runs.
NOTES_FILE=$(mktemp)
if bash scripts/generate-release-notes.sh > "$NOTES_FILE"; then
  echo "=== Generated Release Notes ==="
  cat "$NOTES_FILE"
  echo "==============================="
  echo ""

  # Create annotated tag with the generated notes
  echo "Creating release tag with generated notes..."
  git tag -a "$NEW_TAG" -F "$NOTES_FILE" --cleanup=verbatim

  rm "$NOTES_FILE"
else
  echo "Warning: Failed to generate release notes with AI"
  echo "See the generator's output above for the reason."
  rm "$NOTES_FILE"

  echo ""
  echo "Install at least one AI CLI tool (Claude, Gemini, or Codex)"
  echo "Creating tag with default message..."
  git tag -a "$NEW_TAG" -m "Release $NEW_TAG" --cleanup=verbatim
fi

# Show what will be pushed
echo ""
echo "Ready to push the following:"
[ "$NEW_VERSION" != "$CURRENT_VERSION" ] && echo "  - Commit: chore: release $NEW_TAG"
echo "  - Tag: $NEW_TAG"
echo ""
echo "To push the release, run:"
echo "  git push origin main"
echo "  git push origin $NEW_TAG"
echo ""
echo "Or use the automated script:"
echo "  ./scripts/release-and-push.sh"