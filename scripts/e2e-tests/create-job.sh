#!/bin/bash
# Create and run an IoT Job

set -e

# Check arguments
if [ $# -lt 2 ]; then
    echo "Usage: $0 <thing-name> <template-id>"
    echo ""
    echo "Example:"
    echo "  $0 ihm-dpm-dpm-pi4 get-store-id"
    exit 1
fi

THING_NAME="$1"
TEMPLATE_ID="$2"
REGION="${AWS_REGION:-us-west-2}"
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)

# Generate unique job ID
JOB_ID="${TEMPLATE_ID}-${THING_NAME}-$(date +%s)"

echo "=== Creating IoT Job ==="
echo "Thing Name: ${THING_NAME}"
echo "Template: ${TEMPLATE_ID}"
echo "Job ID: ${JOB_ID}"
echo ""

# Create job
aws iot create-job \
  --job-id "${JOB_ID}" \
  --targets "arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}" \
  --job-template-arn "arn:aws:iot:${REGION}:${ACCOUNT_ID}:jobtemplate/${TEMPLATE_ID}" \
  --region ${REGION}

echo ""
echo "✅ Job created successfully!"
echo "Job ID: ${JOB_ID}"
echo ""
echo "Check job status:"
echo "  ./check-job.sh ${THING_NAME} ${JOB_ID}"
