#!/bin/bash
# Complete release workflow: build, package, upload, create component
# Usage: ./release.sh <version> <s3-bucket>

set -e

if [ $# -lt 2 ]; then
    echo "Usage: $0 <version> <s3-bucket>"
    echo ""
    echo "Example:"
    echo "  $0 1.0.0 my-s3-bucket"
    exit 1
fi

VERSION="$1"
S3_BUCKET="$2"
REGION="${AWS_REGION:-us-west-2}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo "=== Complete Release Workflow ==="
echo "Version: ${VERSION}"
echo "S3 Bucket: ${S3_BUCKET}"
echo "Region: ${REGION}"
echo ""

# Step 1: Build and package
echo "Step 1/4: Building and packaging..."
cd "$PROJECT_ROOT"
bash "$SCRIPT_DIR/build-and-package.sh" "$VERSION"

# Step 2: Upload to S3
echo ""
echo "Step 2/4: Uploading to S3..."
PACKAGE_FILE="device-ops-${VERSION}-aarch64.zip"

if [ ! -f "$PACKAGE_FILE" ]; then
    echo "Error: Package file not found: $PACKAGE_FILE"
    exit 1
fi

aws s3 cp "$PACKAGE_FILE" "s3://${S3_BUCKET}/device-ops/${VERSION}/"
echo "✅ Uploaded to s3://${S3_BUCKET}/device-ops/${VERSION}/${PACKAGE_FILE}"

# Step 3: Create component version
echo ""
echo "Step 3/4: Creating Greengrass component..."
aws greengrassv2 create-component-version \
    --inline-recipe fileb://recipe.yaml \
    --region ${REGION} \
    > /dev/null

echo "✅ Component version ${VERSION} created"

# Step 4: Verify component is deployable
echo ""
echo "Step 4/4: Verifying component..."
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
COMPONENT_ARN="arn:aws:greengrass:${REGION}:${ACCOUNT_ID}:components:com.example.DeviceOps:versions:${VERSION}"

STATUS=$(aws greengrassv2 describe-component \
    --arn "${COMPONENT_ARN}" \
    --region ${REGION} \
    --query 'status.componentState' \
    --output text)

if [ "$STATUS" = "DEPLOYABLE" ]; then
    echo "✅ Component is DEPLOYABLE"
else
    echo "⚠️  Component status: $STATUS"
fi

echo ""
echo "=== Release Complete! ==="
echo ""
echo "Component: com.example.DeviceOps:${VERSION}"
echo "Status: ${STATUS}"
echo ""
echo "Deploy to devices:"
echo "  ./scripts/deploy-to-device.sh <thing-name> ${VERSION}"
echo "  ./scripts/deploy-to-group.sh <group-name> ${VERSION}"
echo ""
echo "Test on device:"
echo "  cd scripts/e2e-tests"
echo "  ./test-multi-step.sh <thing-name> simple"
