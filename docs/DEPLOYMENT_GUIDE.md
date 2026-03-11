# Device Operations Component - Deployment Guide

## Overview

This guide covers the complete deployment process for the Device Operations Component, including cloud infrastructure setup and device deployment.

## Prerequisites

- AWS Account with IoT Core access
- Greengrass Core devices running Greengrass v2 (Lite or Standard)
- AWS CLI configured with appropriate permissions
- S3 bucket for component artifacts

## Part 1: Cloud Infrastructure Setup

### 1.1 Update IoT Policy for Devices

Add the following to your thing certificate policy (or thing group policy):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "iot:Publish"
      ],
      "Resource": [
        "arn:aws:iot:*:*:topic/$aws/things/${iot:Connection.Thing.ThingName}/jobs/$next/get",
        "arn:aws:iot:*:*:topic/$aws/things/${iot:Connection.Thing.ThingName}/jobs/*/update"
      ]
    },
    {
      "Effect": "Allow",
      "Action": [
        "iot:Subscribe"
      ],
      "Resource": [
        "arn:aws:iot:*:*:topicfilter/$aws/things/${iot:Connection.Thing.ThingName}/jobs/notify-next",
        "arn:aws:iot:*:*:topicfilter/$aws/things/${iot:Connection.Thing.ThingName}/jobs/$next/get/accepted",
        "arn:aws:iot:*:*:topicfilter/$aws/things/${iot:Connection.Thing.ThingName}/jobs/+/update/accepted",
        "arn:aws:iot:*:*:topicfilter/$aws/things/${iot:Connection.Thing.ThingName}/jobs/+/update/rejected"
      ]
    },
    {
      "Effect": "Allow",
      "Action": [
        "iot:Receive"
      ],
      "Resource": [
        "arn:aws:iot:*:*:topic/$aws/things/${iot:Connection.Thing.ThingName}/jobs/notify-next",
        "arn:aws:iot:*:*:topic/$aws/things/${iot:Connection.Thing.ThingName}/jobs/$next/get/accepted",
        "arn:aws:iot:*:*:topic/$aws/things/${iot:Connection.Thing.ThingName}/jobs/+/update/accepted",
        "arn:aws:iot:*:*:topic/$aws/things/${iot:Connection.Thing.ThingName}/jobs/+/update/rejected"
      ]
    }
  ]
}
```

**Note**: No custom IoT Core Rules are needed. IoT Jobs `notify-next` handles steady-state delivery; `$next/get` on startup catches jobs queued while offline.

#### Update Existing Policy

```bash
# Get your policy name (usually attached to thing certificate)
POLICY_NAME="YourDevicePolicy"

# Update the policy
aws iot create-policy-version \
  --policy-name ${POLICY_NAME} \
  --policy-document file://device-policy.json \
  --set-as-default
```

### 1.2 Create Custom Job Templates (Optional but Recommended)

Create job templates for common operations:

```bash
# Example: Get Store ID template
cat > get-store-id-template.json << EOF
{
  "document": {
    "version": "1.0",
    "steps": [
      {
        "action": {
          "name": "GetStoreId",
          "type": "runCommand",
          "input": {
            "command": "/opt/device-scripts/get-store-id.sh"
          }
        }
      }
    ]
  },
  "description": "Retrieves store ID from device DNS hints",
  "documentSource": "INLINE"
}
EOF

aws iot create-job-template \
  --job-template-id get-store-id \
  --document-source INLINE \
  --document file://get-store-id-template.json \
  --description "Retrieves store ID from device DNS hints"
```

## Part 2: Component Deployment

### 2.1 Build Component

```bash
# Build for both architectures
./scripts/build-and-package.sh

# Or build for a specific architecture
./scripts/docker-build.sh aarch64
./scripts/package-aarch64.sh

./scripts/docker-build.sh x86_64
./scripts/package-x86_64.sh
```

### 2.2 Upload to S3

```bash
# Set your S3 bucket
S3_BUCKET="your-component-bucket"
COMPONENT_VERSION="1.0.0"

# Upload artifacts for both architectures
aws s3 cp device-ops-${COMPONENT_VERSION}-aarch64.zip \
  s3://${S3_BUCKET}/device-ops/${COMPONENT_VERSION}/
aws s3 cp device-ops-${COMPONENT_VERSION}-x86_64.zip \
  s3://${S3_BUCKET}/device-ops/${COMPONENT_VERSION}/
```

### 2.3 Create Component in Greengrass

```bash
# Update recipe.yaml with your S3 bucket URI
# Then create the component
aws greengrassv2 create-component-version \
  --inline-recipe fileb://recipe.yaml
```

### 2.4 Deploy to Devices

#### Option A: Deploy to Thing Group

```bash
# Create deployment
cat > deployment.json << EOF
{
  "targetArn": "arn:aws:iot:${REGION}:${ACCOUNT_ID}:thinggroup/YourThingGroup",
  "deploymentName": "DeviceOps-Deployment",
  "components": {
    "com.example.DeviceOps": {
      "componentVersion": "1.0.0",
      "configurationUpdate": {
        "merge": "{\"security\":{\"allowlist\":[]},\"execution\":{\"defaultTimeout\":300}}"
      }
    }
  }
}
EOF

aws greengrassv2 create-deployment --cli-input-json file://deployment.json
```

#### Option B: Deploy to Single Device

```bash
cat > deployment.json << EOF
{
  "targetArn": "arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/YourThingName",
  "deploymentName": "DeviceOps-Deployment",
  "components": {
    "com.example.DeviceOps": {
      "componentVersion": "1.0.0",
      "configurationUpdate": {
        "merge": "{\"security\":{\"allowlist\":[]},\"execution\":{\"defaultTimeout\":300}}"
      }
    }
  }
}
EOF

aws greengrassv2 create-deployment --cli-input-json file://deployment.json
```

## Part 3: Device Setup

### 3.1 Install Device Scripts

On each device, install the scripts that jobs will execute:

```bash
# Create scripts directory
sudo mkdir -p /opt/device-scripts

# Copy your scripts
sudo cp examples/get-store-id.sh /opt/device-scripts/
sudo cp examples/get-camera-intrinsics.sh /opt/device-scripts/
sudo cp examples/run-diagnostics.sh /opt/device-scripts/

# Make executable
sudo chmod +x /opt/device-scripts/*.sh
```

### 3.2 Verify Component Installation

```bash
# Check component status
sudo /greengrass/v2/bin/greengrass-cli component list

# View component logs
sudo tail -f /greengrass/v2/logs/com.example.DeviceOps.log
```

Expected log output:
```
[INFO] Device Operations Component starting
[INFO] Connected to Greengrass IPC
[INFO] Subscribing to IoT Jobs notifications
[INFO] Listening for job notifications
```

## Part 4: Testing

### 4.1 Test Single-Step Job

```bash
# Create a test job
THING_NAME="your-thing-name"

cat > test-job.json << EOF
{
  "version": "1.0",
  "includeStdOut": true,
  "steps": [
    {
      "action": {
        "name": "TestCommand",
        "type": "runCommand",
        "input": {
          "command": "/opt/device-scripts/get-store-id.sh"
        }
      }
    }
  ]
}
EOF

aws iot create-job \
  --job-id test-job-$(date +%s) \
  --targets "arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}" \
  --document file://test-job.json
```

### 4.2 Test Multi-Step Job

```bash
cat > multi-step-job.json << EOF
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
EOF

aws iot create-job \
  --job-id multi-step-test-$(date +%s) \
  --targets "arn:aws:iot:${REGION}:${ACCOUNT_ID}:thing/${THING_NAME}" \
  --document file://multi-step-job.json

# Check results
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name ${THING_NAME} \
  --query 'execution.statusDetails.detailsMap.steps' \
  --output text | jq .
```

### 4.3 Test Offline Job Queuing

1. Disconnect device from network
2. Create a job targeting the device
3. Reconnect device
4. Verify job executes automatically (IoT Jobs `notify-next` delivers it on reconnection, or startup `$next/get` query picks it up)

## Part 5: Monitoring

### 5.1 CloudWatch Metrics

Monitor job execution metrics:

```bash
# View job execution metrics
aws cloudwatch get-metric-statistics \
  --namespace AWS/IoT \
  --metric-name JobExecutionSucceeded \
  --dimensions Name=JobId,Value=your-job-id \
  --start-time $(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S) \
  --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
  --period 300 \
  --statistics Sum
```

### 5.2 Component Logs

```bash
# View component logs
sudo /greengrass/v2/bin/greengrass-cli logs get \
  --log-file com.example.DeviceOps.log \
  --follow

# View Greengrass system logs
sudo tail -f /greengrass/v2/logs/greengrass.log
```

## Troubleshooting

### Issue: Jobs not executing

**Check:**
1. Component is running: `sudo /greengrass/v2/bin/greengrass-cli component list`
2. IoT policy allows job topics
3. Scripts exist and are executable: `ls -la /opt/device-scripts/`
4. Component logs for errors

### Issue: Permission denied errors

**Check:**
1. Recipe.yaml has correct accessControl configuration
2. Component has IPC permissions for MQTT proxy
3. IoT policy attached to device certificate

## Configuration Options

### Security Controls

Enable command allowlisting:

```json
{
  "security": {
    "allowlist": [
      "/opt/device-scripts/",
      "hostname"
    ]
  }
}
```

### Execution Settings

Adjust default timeout:

```json
{
  "execution": {
    "defaultTimeout": 600
  }
}
```

## Best Practices

1. **Use Job Templates**: Define templates for common operations with IAM restrictions
2. **Enable Security Controls**: Use allowlisting in production environments
3. **Monitor Logs**: Set up CloudWatch Logs forwarding for centralized monitoring
4. **Test Offline Recovery**: Verify jobs execute after reconnection
5. **Version Scripts**: Keep device scripts in version control
6. **Gradual Rollout**: Deploy to small groups first, then expand

## Support

For issues or questions:
- Check component logs: `/greengrass/v2/logs/com.example.DeviceOps.log`
- Review AWS IoT Jobs documentation: https://docs.aws.amazon.com/iot/latest/developerguide/iot-jobs.html
- Check Greengrass documentation: https://docs.aws.amazon.com/greengrass/v2/developerguide/
