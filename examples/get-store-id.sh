#!/bin/bash
# Example device script: Get store ID from DNS hints
# This should be installed at /opt/device-scripts/get-store-id.sh on the device

set -e

# Query DNS for store ID TXT record
STORE_ID=$(nslookup -type=TXT store-id 10.255.255.1:53 2>/dev/null | grep "text =" | cut -d'"' -f2)

if [ -z "$STORE_ID" ]; then
    echo "Error: Could not retrieve store ID" >&2
    exit 1
fi

# Output result as JSON
cat <<EOF
{
  "storeId": "$STORE_ID",
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "hostname": "$(hostname)"
}
EOF

exit 0
