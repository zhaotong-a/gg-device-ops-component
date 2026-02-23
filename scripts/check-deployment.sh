#!/bin/bash
# Check deployment status

set -e

# Check arguments
if [ $# -lt 1 ]; then
    echo "Usage: $0 <deployment-id|thing-name>"
    echo ""
    echo "Examples:"
    echo "  $0 a1b2c3d4-5678-90ab-cdef-1234567890ab  # Check by deployment ID"
    echo "  $0 my-device                              # Check by thing name"
    exit 1
fi

INPUT="$1"
REGION="${AWS_REGION:-us-west-2}"
COMPONENT_NAME="com.example.DeviceOps"

# Check if input looks like a deployment ID (UUID format)
if [[ "$INPUT" =~ ^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$ ]]; then
    # Input is a deployment ID
    DEPLOYMENT_ID="$INPUT"
    
    echo "=== Checking Deployment Status ==="
    echo "Deployment ID: ${DEPLOYMENT_ID}"
    echo ""
    
    aws greengrassv2 get-deployment \
        --deployment-id "${DEPLOYMENT_ID}" \
        --region ${REGION} \
        --query '{
            Status: deploymentStatus,
            CreatedAt: creationTimestamp,
            Components: components,
            TargetArn: targetArn
        }' \
        --output table
else
    # Input is a thing name
    THING_NAME="$INPUT"
    
    echo "=== Checking Component Status on Device ==="
    echo "Thing Name: ${THING_NAME}"
    echo ""
    
    # List effective deployments for the thing
    echo "Effective deployments:"
    aws greengrassv2 list-effective-deployments \
        --core-device-thing-name "${THING_NAME}" \
        --region ${REGION} \
        --query 'effectiveDeployments[*].{
            DeploymentId: deploymentId,
            Status: coreDeviceExecutionStatus,
            CreatedAt: creationTimestamp,
            Reason: statusDetails.errorStack[0]
        }' \
        --output table
    
    echo ""
    echo "Installed components:"
    aws greengrassv2 list-installed-components \
        --core-device-thing-name "${THING_NAME}" \
        --region ${REGION} \
        --query "installedComponents[?componentName=='${COMPONENT_NAME}'].{
            Name: componentName,
            Version: componentVersion,
            State: lifecycleState,
            Status: lifecycleStatusCodes[0]
        }" \
        --output table
    
    echo ""
    echo "To view logs on device:"
    echo "  ssh user@${THING_NAME}"
    echo "  tail -f /greengrass/v2/logs/${COMPONENT_NAME}.log"
fi
