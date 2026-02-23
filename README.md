# Device Operations Component

**Version 1.0.0** - Lightweight Greengrass component for executing AWS IoT Jobs on edge devices.

## Features

- Execute pre-installed bash scripts via AWS IoT Jobs
- **Multi-step jobs** with sequential execution
- **Failure handling** with `ignoreStepFailure` and `allowStdErr`
- **Final step** execution for cleanup/summary tasks
- Automatic reconnection detection and job recovery
- IAM-based security with job template restrictions
- Optional command allowlisting for defense-in-depth
- Works with Greengrass Nucleus Lite
- ~1.1MB binary, <20MB memory, <2s job latency

**Architecture:** Cloud (IoT Jobs) → Greengrass IPC → Device Ops → Bash Scripts

**Reconnection Handling:** Automatically detects device reconnections and queries for missed jobs using IoT Core Rules. See [docs/DEPLOYMENT_GUIDE.md](docs/DEPLOYMENT_GUIDE.md) for setup.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for technical details.

## Quick Start

### Prerequisites

1. **Cloud Setup (One-time):** Set up IoT Core Rule for reconnection detection
   ```bash
   # See DEPLOYMENT_GUIDE.md for detailed instructions
   # Creates rule to detect device reconnections
   ```

2. **Device Scripts:** Install scripts that jobs will execute
   ```bash
   sudo mkdir -p /opt/device-scripts
   sudo cp examples/*.sh /opt/device-scripts/
   sudo chmod +x /opt/device-scripts/*.sh
   ```

### Build & Deploy

```bash
# 1. Build and package
./scripts/build-and-package.sh

# 2. Complete release (build, package, upload, create component)
./scripts/release.sh 1.0.0 my-s3-bucket

# 3. Deploy to device
./scripts/deploy-to-device.sh my-device 1.0.0

# 4. Deploy and test (all-in-one)
./scripts/deploy-and-test.sh my-device 1.0.0 simple
```

**Individual Steps:**
```bash
# Build only
./scripts/docker-build.sh aarch64

# Package only
./scripts/package-aarch64.sh

# Deploy to group
./scripts/deploy-to-group.sh my-group 1.0.0
```

**Build Requirements:** Docker (recommended) or Rust + zig + cargo-zigbuild  
**Test:** `./scripts/docker-build.sh` (includes tests)

## Configuration

Config file: `/greengrass/v2/config/device-ops-config.json`

```json
{
  "security": {
    "enabled": false,
    "commandAllowlist": ["/opt/device-scripts/get-store-id.sh"],
    "pathAllowlist": ["/opt/device-scripts/"]
  },
  "execution": {
    "defaultTimeout": 300
  }
}
```

## Usage

### Single-Step Job

**1. Create job template:**
```bash
aws iot create-job-template \
  --job-template-id get-store-id \
  --document '{
    "version": "1.0",
    "includeStdOut": true,
    "steps": [{
      "action": {
        "name": "Get-Store-ID",
        "type": "runCommand",
        "input": {
          "command": "/opt/device-scripts/get-store-id.sh",
          "timeout": 60
        }
      }
    }]
  }'
```

### Multi-Step Job

Execute multiple commands sequentially:

```json
{
  "version": "1.0",
  "includeStdOut": true,
  "steps": [
    {
      "action": {
        "name": "Step1-GetStoreID",
        "type": "runCommand",
        "input": {
          "command": "/opt/device-scripts/get-store-id.sh",
          "timeout": 30
        }
      }
    },
    {
      "action": {
        "name": "Step2-RunDiagnostics",
        "type": "runCommand",
        "input": {
          "command": "/opt/device-scripts/run-diagnostics.sh",
          "timeout": 60
        }
      }
    }
  ],
  "finalStep": {
    "action": {
      "name": "Cleanup",
      "type": "runCommand",
      "input": {
        "command": "/bin/echo",
        "args": ["All steps completed"],
        "timeout": 5
      }
    }
  }
}
```

### Failure Handling

**Ignore step failures:**
```json
{
  "action": {
    "name": "OptionalStep",
    "type": "runCommand",
    "input": {
      "command": "/opt/device-scripts/optional.sh"
    },
    "ignoreStepFailure": true
  }
}
```

**Allow stderr output:**
```json
{
  "action": {
    "name": "StepWithWarnings",
    "type": "runCommand",
    "input": {
      "command": "/opt/device-scripts/script.sh"
    },
    "allowStdErr": 5
  }
}
```

**Key Points:**
- Steps execute sequentially
- Execution stops on first failure (unless `ignoreStepFailure: true`)
- `finalStep` only runs if all steps succeed
- Set `includeStdOut: true` to capture command output
- Multi-step results are in compact JSON format

### Run & Check Results

**2. Run job:**
```bash
aws iot create-job \
  --job-id get-store-id-$(date +%s) \
  --targets arn:aws:iot:us-west-2:123456789012:thing/my-device \
  --job-template-arn arn:aws:iot:us-west-2:123456789012:jobtemplate/get-store-id
```

**3. Get results:**
```bash
# Single-step job
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name my-device \
  --query 'execution.statusDetails.detailsMap.stdout'

# Multi-step job (parse JSON)
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name my-device \
  --query 'execution.statusDetails.detailsMap.steps' \
  --output text | jq .
```

**Logs:** `/greengrass/v2/logs/com.example.DeviceOps.log`

## Troubleshooting

**Component not starting:**
```bash
tail -f /greengrass/v2/logs/com.example.DeviceOps.log
sudo /greengrass/v2/bin/greengrass-cli component list
```

**Jobs not received:**
```bash
aws iot list-job-executions-for-thing --thing-name <thing-name>
```

**Script fails:**
```bash
chmod +x /opt/device-scripts/*.sh
ls -la /opt/device-scripts/
```

## Security

**IAM Policy** - Restrict to specific job templates:
```json
{
  "Effect": "Allow",
  "Action": "iot:CreateJob",
  "Resource": "arn:aws:iot:us-west-2:123456789012:job/*",
  "Condition": {
    "StringEquals": {
      "iot:JobTemplate": [
        "arn:aws:iot:us-west-2:123456789012:jobtemplate/get-store-id"
      ]
    }
  }
}
```

**Best Practices:**
- Use job templates with hardcoded commands
- Restrict IAM policies to specific templates
- Enable command allowlisting for defense-in-depth (optional)
- Run as non-root user when possible

## More Info

- **[docs/DEPLOYMENT_GUIDE.md](docs/DEPLOYMENT_GUIDE.md)** - Complete deployment guide with multi-step examples
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** - System architecture and technical details
- **[docs/TESTING_GUIDE.md](docs/TESTING_GUIDE.md)** - Testing procedures and structure
- **[docs/HOW_TO_CHECK_JOB_OUTPUT.md](docs/HOW_TO_CHECK_JOB_OUTPUT.md)** - Viewing single and multi-step job results
- **[CHANGELOG.md](CHANGELOG.md)** - Version history
- **examples/** - Sample device scripts (install to `/opt/device-scripts/`)
- **job-templates/** - Example job templates for testing
- **scripts/** - Build and deployment scripts
- **scripts/e2e-tests/** - End-to-end tests for real devices

**Cost:** $0.0025/job + minimal reconnection overhead (~$3.60/month for 1000 devices)
