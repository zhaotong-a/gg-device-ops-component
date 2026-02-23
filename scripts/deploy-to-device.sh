#!/bin/bash
# Deploy component to a specific Greengrass device

set -e

# Check arguments
if [ $# -lt 1 ]; then
    echo "Usage: $0 <thing-name> [component-version]"
    echo ""
    echo "Example:"
    echo "  $0 my-device"
    echo "  $0 my-device 1.0.0"
    exit 1
fi

THING_NAME="$1"
VERSION="${2:-1.0.0}"
COMPONENT_NAME="com.example.DeviceOps"
REGION="${AWS_REGION:-us-west-2}"
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

echo "=== Deploying to Device ==="
echo "Thing Name: ${THING_NAME}"
echo "Component: ${COMPONENT_NAME}"
echo "Version: ${VERSION}"
echo "Region: ${REGION}"
echo ""

# Get thing ARN
THING_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}"

# Create deployment
echo "Creating deployment..."
DEPLOYMENT_ID=$(aws greengrassv2 create-deployment \
    --target-arn "${THING_ARN}" \
    --deployment-name "Deploy-${COMPONENT_NAME}-${VERSION}-$(date +%Y%m%d-%H%M%S)" \
    --components "{
        \"${COMPONENT_NAME}\": {
            \"componentVersion\": \"${VERSION}\",
            \"configurationUpdate\": {
                \"merge\": \"{\\\"security\\\":{\\\"enabled\\\":false},\\\"execution\\\":{\\\"defaultTimeout\\\":300}}\"
            }
        }
    }" \
    --region ${REGION} \
    --query 'deploymentId' \
    --output text)

echo ""
echo "✅ Deployment created successfully!"
echo "Deployment ID: ${DEPLOYMENT_ID}"
echo ""
echo "Check deployment status:"
echo "  aws greengrassv2 get-deployment --deployment-id ${DEPLOYMENT_ID} --region ${REGION}"
echo ""
echo "Monitor on device:"
echo "  ssh user@device"
echo "  sudo /greengrass/v2/bin/greengrass-cli component list"
echo "  tail -f /greengrass/v2/logs/${COMPONENT_NAME}.log"
