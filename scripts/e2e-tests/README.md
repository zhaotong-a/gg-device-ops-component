# End-to-End Tests

These scripts test the component on real devices with AWS IoT Jobs integration.

## Prerequisites

- Component deployed to a device or device group
- AWS CLI configured with IoT permissions
- Device online and connected to Greengrass

## Test Scripts

### test-multi-step.sh
Test multi-step job execution with different scenarios.

```bash
# Simple multi-step (basic Linux commands)
./test-multi-step.sh <thing-name> simple

# Device scripts multi-step (requires scripts installed)
./test-multi-step.sh <thing-name> device

# Failure handling test (ignoreStepFailure, allowStdErr)
./test-multi-step.sh <thing-name> failure
```

**Examples:**
```bash
./test-multi-step.sh ihm-dpm-dpm-pi-5 simple
./test-multi-step.sh ihm-dpm-dpm-pi-5 failure
```

### create-job.sh
Create a job from a template.

```bash
./create-job.sh <thing-name> <template-name>
```

**Examples:**
```bash
./create-job.sh my-device get-store-id
```

### check-job.sh
Check job execution status and output.

```bash
./check-job.sh <thing-name> <job-id>
```

**Examples:**
```bash
./check-job.sh my-device multi-step-simple-my-device-1771872992
```

## Job Templates

Templates are in `../../job-templates/`:
- `simple-multi-step.json` - Basic system commands
- `multi-step-test.json` - Device scripts (requires installation)
- `multi-step-with-failure-handling.json` - Failure handling demo
- `get-store-id-with-output.json` - Single-step example

## Quick Test Flow

1. **Deploy component** (if not already deployed):
   ```bash
   cd ../..
   ./scripts/deploy-to-device.sh my-device 1.0.0
   ```

2. **Run simple multi-step test**:
   ```bash
   cd scripts/e2e-tests
   ./test-multi-step.sh my-device simple
   ```

3. **Check results**:
   ```bash
   # Wait a few seconds, then check
   aws iot describe-job-execution \
     --job-id <job-id-from-output> \
     --thing-name my-device \
     --query 'execution.statusDetails.detailsMap.steps' \
     --output text | jq .
   ```

4. **Test failure handling**:
   ```bash
   ./test-multi-step.sh my-device failure
   ```

## Expected Results

### Simple Test
- ✅ All 5 steps succeed
- ✅ Final step executes
- ✅ Output captured for each step

### Failure Test
- ✅ Step 2 fails but is ignored
- ✅ Execution continues
- ✅ Final step runs
- ✅ Overall status: SUCCESS

## Troubleshooting

**Job not executing:**
```bash
# Check component logs on device
ssh user@device
sudo tail -f /greengrass/v2/logs/com.example.DeviceOps.log
```

**Job failed:**
```bash
# Check job status
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name <thing-name> \
  --query 'execution.statusDetails.detailsMap'
```

**Device scripts missing:**
```bash
# Install scripts on device
ssh user@device
sudo mkdir -p /opt/device-scripts
sudo cp /path/to/scripts/*.sh /opt/device-scripts/
sudo chmod +x /opt/device-scripts/*.sh
```
