#!/bin/bash
# Package x86_64 binary for deployment

set -e

# Get the project root directory (parent of scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Change to project root
cd "$PROJECT_ROOT"

VERSION=$(grep '^version = ' "$PROJECT_ROOT/Cargo.toml" | head -1 | cut -d'"' -f2)
ARCH="x86_64"
PACKAGE_NAME="device-ops-${VERSION}-${ARCH}"

echo "Packaging ${PACKAGE_NAME}..."

# Create package directory structure
rm -rf package-${ARCH}
mkdir -p package-${ARCH}/bin
cp target/x86_64-unknown-linux-gnu/release/device-ops-component package-${ARCH}/bin/
cp config.json package-${ARCH}/

# Create zip file
cd package-${ARCH}
zip -r ../${PACKAGE_NAME}.zip .
cd ..

echo ""
echo "✅ Package created: ${PACKAGE_NAME}.zip"
ls -lh ${PACKAGE_NAME}.zip

echo ""
echo "To upload to S3:"
echo "  aws s3 cp ${PACKAGE_NAME}.zip s3://YOUR-BUCKET/device-ops/${VERSION}/"
