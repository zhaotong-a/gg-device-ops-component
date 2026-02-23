#!/bin/bash
# Check IoT Job status and results

set -e

# Check arguments
if [ $# -lt 2 ]; then
    echo "Usage: $0 <thing-name> <job-id>"
    echo ""
    echo "Example:"
    echo "  $0 ihm-dpm-dpm-pi4 get-store-id-ihm-dpm-dpm-pi4-1234567890"
    exit 1
fi

THING_NAME="$1"
JOB_ID="$2"
REGION="${AWS_REGION:-us-west-2}"

echo "=== Checking Job Status ==="
echo "Thing Name: ${THING_NAME}"
echo "Job ID: ${JOB_ID}"
echo ""

# Get job execution details
aws iot describe-job-execution \
  --job-id "${JOB_ID}" \
  --thing-name "${THING_NAME}" \
  --region ${REGION} \
  --query '{
    Status: execution.status,
    QueuedAt: execution.queuedAt,
    StartedAt: execution.startedAt,
    LastUpdatedAt: execution.lastUpdatedAt,
    StatusDetails: execution.statusDetails
  }' \
  --output table

echo ""
echo "To see full output:"
echo "  aws iot describe-job-execution --job-id ${JOB_ID} --thing-name ${THING_NAME} --region ${REGION}"
