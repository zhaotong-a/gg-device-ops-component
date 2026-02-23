#!/bin/bash
# Deploy component and run E2E tests
# Usage: ./deploy-and-test.sh <thing-name> <version> [test-type]

set -e

if [ $# -lt 2 ]; then
    echo "Usage: $0 <thing-name> <version> [test-type]"
    echo ""
    echo "Test types:"
    echo "  simple  - Simple multi-step test (default)"
    echo "  failure - Failure handling test"
    echo "  device  - Device scripts test"
    echo ""
    echo "Example:"
    echo "  $0 my-device 1.0.0"
    echo "  $0 my-device 1.0.0 failure"
    exit 1
fi

THING_NAME="$1"
VERSION="$2"
TEST_TYPE="${3:-simple}"
REGION="${AWS_REGION:-us-west-2}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Deploy and Test ==="
echo "Thing: ${THING_NAME}"
echo "Version: ${VERSION}"
echo "Test: ${TEST_TYPE}"
echo ""

# Step 1: Deploy
echo "Step 1/3: Deploying component..."
bash "$SCRIPT_DIR/deploy-to-device.sh" "$THING_NAME" "$VERSION"

# Step 2: Wait for deployment
echo ""
echo "Step 2/3: Waiting for deployment (30 seconds)..."
sleep 30

# Step 3: Check deployment status
echo ""
echo "Checking deployment status..."
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
THING_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}"

DEPLOYMENT_ID=$(aws greengrassv2 list-effective-deployments \
    --core-device-thing-name "${THING_NAME}" \
    --region ${REGION} \
    --query 'effectiveDeployments[0].deploymentId' \
    --output text 2>/dev/null || echo "")

if [ -n "$DEPLOYMENT_ID" ] && [ "$DEPLOYMENT_ID" != "None" ]; then
    STATUS=$(aws greengrassv2 get-deployment \
        --deployment-id "${DEPLOYMENT_ID}" \
        --region ${REGION} \
        --query 'deploymentStatus' \
        --output text)
    echo "Deployment status: ${STATUS}"
fi

# Step 4: Run E2E test
echo ""
echo "Step 3/3: Running E2E test..."
cd "$SCRIPT_DIR/e2e-tests"
bash test-multi-step.sh "$THING_NAME" "$TEST_TYPE"

echo ""
echo "=== Deploy and Test Complete! ==="
echo ""
echo "Check device logs:"
echo "  ssh user@device"
echo "  sudo tail -f /greengrass/v2/logs/com.example.DeviceOps.log"
