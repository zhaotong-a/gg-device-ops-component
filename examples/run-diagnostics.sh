#!/bin/bash
# Example device script: Run system diagnostics
# This should be installed at /opt/device-scripts/run-diagnostics.sh on the device

set -e

# Collect system information
UPTIME=$(uptime -p)
MEMORY=$(free -h | grep Mem | awk '{print $3 "/" $2}')
DISK=$(df -h / | tail -1 | awk '{print $3 "/" $2 " (" $5 " used)"}')
CPU_TEMP=$(cat /sys/class/thermal/thermal_zone0/temp 2>/dev/null | awk '{print $1/1000 "°C"}' || echo "N/A")

# Output diagnostics as JSON
cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "hostname": "$(hostname)",
  "uptime": "$UPTIME",
  "memory": "$MEMORY",
  "disk": "$DISK",
  "cpuTemp": "$CPU_TEMP",
  "loadAverage": "$(uptime | awk -F'load average:' '{print $2}')"
}
EOF

exit 0
