#!/bin/bash
# Remove component from a Greengrass thing group

set -e

# Check arguments
if [ $# -lt 1 ]; then
    echo "Usage: $0 <thing-group-name>"
    echo ""
    echo "Example:"
    echo "  $0 zhatong-test"
    exit 1
fi

THING_GROUP="$1"
COMPONENT_NAME="com.example.DeviceOps"
REGION="${AWS_REGION:-us-west-2}"
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

echo "=== Removing Component from Thing Group ==="
echo "Thing Group: ${THING_GROUP}"
echo "Component: ${COMPONENT_NAME}"
echo "Region: ${REGION}"
echo ""

# Get thing group ARN
THING_GROUP_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thinggroup/${THING_GROUP}"

# Create deployment without the component
echo "Creating fleet deployment to remove component..."
DEPLOYMENT_ID=$(aws greengrassv2 create-deployment \
    --target-arn "${THING_GROUP_ARN}" \
    --deployment-name "Fleet-Remove-${COMPONENT_NAME}-$(date +%Y%m%d-%H%M%S)" \
    --components "{}" \
    --components-to-remove "[\"${COMPONENT_NAME}\"]" \
    --region ${REGION} \
    --query 'deploymentId' \
    --output text)

echo ""
echo "✅ Fleet removal deployment created successfully!"
echo "Deployment ID: ${DEPLOYMENT_ID}"
echo ""
echo "Check deployment status:"
echo "  aws greengrassv2 get-deployment --deployment-id ${DEPLOYMENT_ID} --region ${REGION}"
echo ""
echo "List devices in group:"
echo "  aws iot list-things-in-thing-group --thing-group-name ${THING_GROUP} --region ${REGION}"
