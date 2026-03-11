#!/bin/bash
# Build and package the component for all supported architectures
# Usage: ./build-and-package.sh [version]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="${1:-$(grep '^version = ' "$PROJECT_ROOT/Cargo.toml" | cut -d'"' -f2)}"

echo "=== Build and Package ==="
echo "Version: ${VERSION}"
echo ""

# Step 1: Build both architectures
echo "Step 1/2: Building binaries..."
cd "$PROJECT_ROOT"
bash "$SCRIPT_DIR/docker-build.sh" all

# Step 2: Package both
echo ""
echo "Step 2/2: Creating packages..."
bash "$SCRIPT_DIR/package-aarch64.sh"
bash "$SCRIPT_DIR/package-x86_64.sh"

echo ""
echo "✅ Build and package complete!"
echo ""
echo "Packages:"
echo "  device-ops-${VERSION}-aarch64.zip"
echo "  device-ops-${VERSION}-x86_64.zip"
echo ""
echo "Next steps:"
echo "  # Upload to S3"
echo "  aws s3 cp device-ops-${VERSION}-aarch64.zip s3://your-bucket/device-ops/${VERSION}/"
echo "  aws s3 cp device-ops-${VERSION}-x86_64.zip s3://your-bucket/device-ops/${VERSION}/"
echo ""
echo "  # Create component"
echo "  aws greengrassv2 create-component-version --inline-recipe fileb://recipe.yaml"
echo ""
echo "  # Deploy"
echo "  ./scripts/deploy-to-device.sh <thing-name> ${VERSION}"
echo "  ./scripts/deploy-to-group.sh <group-name> ${VERSION}"
