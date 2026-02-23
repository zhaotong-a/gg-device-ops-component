#!/bin/bash
# Remove component from a specific Greengrass device

set -e

# Check arguments
if [ $# -lt 1 ]; then
    echo "Usage: $0 <thing-name>"
    echo ""
    echo "Example:"
    echo "  $0 ihm-dpm-dpm-pi4"
    exit 1
fi

THING_NAME="$1"
COMPONENT_NAME="com.example.DeviceOps"
REGION="${AWS_REGION:-us-west-2}"
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

echo "=== Removing Component from Device ==="
echo "Thing Name: ${THING_NAME}"
echo "Component: ${COMPONENT_NAME}"
echo "Region: ${REGION}"
echo ""

# Get thing ARN
THING_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}"

# Create deployment without the component
echo "Creating deployment to remove component..."
DEPLOYMENT_ID=$(aws greengrassv2 create-deployment \
    --target-arn "${THING_ARN}" \
    --deployment-name "Remove-${COMPONENT_NAME}-$(date +%Y%m%d-%H%M%S)" \
    --components "{}" \
    --components-to-remove "[\"${COMPONENT_NAME}\"]" \
    --region ${REGION} \
    --query 'deploymentId' \
    --output text)

echo ""
echo "✅ Removal deployment created successfully!"
echo "Deployment ID: ${DEPLOYMENT_ID}"
echo ""
echo "Check deployment status:"
echo "  aws greengrassv2 get-deployment --deployment-id ${DEPLOYMENT_ID} --region ${REGION}"
echo ""
echo "Monitor on device:"
echo "  ssh user@device"
echo "  sudo /greengrass/v2/bin/greengrass-cli component list"
