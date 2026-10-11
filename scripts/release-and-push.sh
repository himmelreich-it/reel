#!/usr/bin/env bash
set -e

echo "Reel Release & Push Automation"
echo "==============================="
echo ""

# Run the make-release script
bash scripts/make-release.sh "$@"

# The tag make-release.sh just created. A hit release can share its commit with
# the upstream tag, so take the newest tag on HEAD rather than git describe.
NEW_VERSION=$(git tag --points-at HEAD --sort=-creatordate | head -1)

echo "Pushing release to origin..."
git push origin main
git push origin "$NEW_VERSION"

echo ""
echo "✅ Release $NEW_VERSION successfully pushed!"
echo ""
echo "Automated release workflow triggered!"
echo "  - GitHub Actions will build packages for Linux (x86_64 and ARM64)"
echo "  - Release notes will be auto-generated from commits"
if [[ "$NEW_VERSION" =~ -(alpha|beta|rc|dev|pre) ]]; then
  echo "  - Release will be marked as DRAFT (prerelease detected)"
  echo "  - Review and publish manually when ready"
else
  echo "  - Release will be PUBLISHED automatically"
fi
echo ""
echo "Monitor progress at:"
echo "  https://github.com/himmelreich-it/reel/actions"