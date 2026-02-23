#!/bin/bash
# Deploy component to a Greengrass thing group (fleet deployment)

set -e

# Check arguments
if [ $# -lt 1 ]; then
    echo "Usage: $0 <thing-group-name> [component-version]"
    echo ""
    echo "Example:"
    echo "  $0 retail-stores"
    echo "  $0 retail-stores 1.0.0"
    exit 1
fi

THING_GROUP="$1"
VERSION="${2:-1.0.0}"
COMPONENT_NAME="com.example.DeviceOps"
REGION="${AWS_REGION:-us-west-2}"
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

echo "=== Deploying to Thing Group ==="
echo "Thing Group: ${THING_GROUP}"
echo "Component: ${COMPONENT_NAME}"
echo "Version: ${VERSION}"
echo "Region: ${REGION}"
echo ""

# Get thing group ARN
THING_GROUP_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thinggroup/${THING_GROUP}"

# Create deployment with rollout configuration
echo "Creating fleet deployment..."
DEPLOYMENT_ID=$(aws greengrassv2 create-deployment \
    --target-arn "${THING_GROUP_ARN}" \
    --deployment-name "Fleet-Deploy-${COMPONENT_NAME}-${VERSION}-$(date +%Y%m%d-%H%M%S)" \
    --components "{
        \"${COMPONENT_NAME}\": {
            \"componentVersion\": \"${VERSION}\",
            \"configurationUpdate\": {
                \"merge\": \"{\\\"security\\\":{\\\"enabled\\\":false},\\\"execution\\\":{\\\"defaultTimeout\\\":300}}\"
            }
        }
    }" \
    --iot-job-configuration "{
        \"jobExecutionsRolloutConfig\": {
            \"maximumPerMinute\": 10,
            \"exponentialRate\": {
                \"baseRatePerMinute\": 5,
                \"incrementFactor\": 2,
                \"rateIncreaseCriteria\": {
                    \"numberOfSucceededThings\": 8
                }
            }
        },
        \"abortConfig\": {
            \"criteriaList\": [
                {
                    \"failureType\": \"FAILED\",
                    \"action\": \"CANCEL\",
                    \"thresholdPercentage\": 10,
                    \"minNumberOfExecutedThings\": 10
                }
            ]
        }
    }" \
    --region ${REGION} \
    --query 'deploymentId' \
    --output text)

echo ""
echo "✅ Fleet deployment created successfully!"
echo "Deployment ID: ${DEPLOYMENT_ID}"
echo ""
echo "Rollout configuration:"
echo "  - Maximum per minute: 10 devices"
echo "  - Base rate: 5 devices/minute"
echo "  - Abort if >10% fail (min 10 devices)"
echo ""
echo "Check deployment status:"
echo "  aws greengrassv2 get-deployment --deployment-id ${DEPLOYMENT_ID} --region ${REGION}"
echo ""
echo "List devices in group:"
echo "  aws iot list-things-in-thing-group --thing-group-name ${THING_GROUP} --region ${REGION}"
