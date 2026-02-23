#!/bin/bash
# Test multi-step job execution

set -e

if [ $# -lt 2 ]; then
    echo "Usage: $0 <thing-name> <test-type>"
    echo ""
    echo "Test types:"
    echo "  simple       - Simple 4-step diagnostic job (hostname, date, uptime, disk)"
    echo "  device       - Multi-step using device scripts (requires scripts installed)"
    echo "  failure      - Test failure handling with ignoreStepFailure"
    echo ""
    echo "Example:"
    echo "  $0 ihm-dpm-dpm-pi-5 simple"
    exit 1
fi

THING_NAME="$1"
TEST_TYPE="$2"
REGION="${AWS_REGION:-us-west-2}"

case "$TEST_TYPE" in
    simple)
        TEMPLATE_FILE="job-templates/simple-multi-step.json"
        JOB_PREFIX="multi-step-simple"
        ;;
    device)
        TEMPLATE_FILE="job-templates/multi-step-test.json"
        JOB_PREFIX="multi-step-device"
        ;;
    failure)
        TEMPLATE_FILE="job-templates/multi-step-with-failure-handling.json"
        JOB_PREFIX="multi-step-failure"
        ;;
    *)
        echo "Error: Unknown test type: $TEST_TYPE"
        echo "Valid types: simple, device, failure"
        exit 1
        ;;
esac

if [ ! -f "$TEMPLATE_FILE" ]; then
    echo "Error: Template file not found: $TEMPLATE_FILE"
    exit 1
fi

JOB_ID="${JOB_PREFIX}-${THING_NAME}-$(date +%s)"
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
TARGET_ARN="arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}"

echo "=== Creating Multi-Step Test Job ==="
echo "Thing Name: ${THING_NAME}"
echo "Test Type: ${TEST_TYPE}"
echo "Job ID: ${JOB_ID}"
echo "Template: ${TEMPLATE_FILE}"
echo ""

# Create the job
aws iot create-job \
    --job-id "${JOB_ID}" \
    --targets "${TARGET_ARN}" \
    --document file://${TEMPLATE_FILE} \
    --region ${REGION} \
    --timeout-config inProgressTimeoutInMinutes=5 \
    > /dev/null

echo "✅ Job created successfully!"
echo ""
echo "Wait a few seconds for execution, then check output:"
echo ""
echo "  # Check job status"
echo "  aws iot describe-job-execution \\"
echo "    --job-id ${JOB_ID} \\"
echo "    --thing-name ${THING_NAME} \\"
echo "    --region ${REGION}"
echo ""
echo "  # Get all step outputs"
echo "  aws iot describe-job-execution \\"
echo "    --job-id ${JOB_ID} \\"
echo "    --thing-name ${THING_NAME} \\"
echo "    --region ${REGION} \\"
echo "    --query 'execution.statusDetails.detailsMap' \\"
echo "    --output json | jq ."
echo ""
echo "  # Get specific step output"
echo "  aws iot describe-job-execution \\"
echo "    --job-id ${JOB_ID} \\"
echo "    --thing-name ${THING_NAME} \\"
echo "    --region ${REGION} \\"
echo "    --query 'execution.statusDetails.detailsMap.step_1_stdout' \\"
echo "    --output text"
echo ""
echo "Monitor on device:"
echo "  sudo journalctl -u greengrass.service -f | grep '${JOB_ID}'"
