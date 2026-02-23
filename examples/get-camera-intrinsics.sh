#!/bin/bash
# Example device script: Get camera device intrinsics
# This should be installed at /opt/device-scripts/get-camera-intrinsics.sh on the device

set -e

INTRINSICS_FILE="/opt/camera/intrinsics.json"

if [ ! -f "$INTRINSICS_FILE" ]; then
    echo "Error: Camera intrinsics file not found: $INTRINSICS_FILE" >&2
    exit 1
fi

# Read and output camera intrinsics
cat "$INTRINSICS_FILE"

exit 0
