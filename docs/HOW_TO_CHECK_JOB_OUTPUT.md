# How to Check Job Output

## Quick Reference

### Single-Step Job Output

```bash
# Get stdout
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name <thing-name> \
  --region us-west-2 \
  --query 'execution.statusDetails.detailsMap.stdout' \
  --output text
```

### Multi-Step Job Output

```bash
# Get all steps (returns JSON array)
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name <thing-name> \
  --region us-west-2 \
  --query 'execution.statusDetails.detailsMap.steps' \
  --output text | jq .
```

## Output Format

### Single-Step Format
```json
{
  "steps_executed": "1",
  "overall_success": "true",
  "step_name": "Get-Store-ID",
  "exit_code": "0",
  "execution_time_ms": "3",
  "stdout": "{\"storeId\":\"STORE-123\"}"
}
```

### Multi-Step Format
```json
{
  "steps_executed": "5",
  "overall_success": "true",
  "steps": "[{\"name\":\"Step1\",\"exit_code\":0,\"stdout\":\"...\"}...]"
}
```

The `steps` field contains a JSON array string. Parse it with `jq`:

```bash
# Pretty print all steps
aws iot describe-job-execution ... \
  --query 'execution.statusDetails.detailsMap.steps' \
  --output text | jq .

# Get specific step output
aws iot describe-job-execution ... \
  --query 'execution.statusDetails.detailsMap.steps' \
  --output text | jq '.[0].stdout'
```

## Enable Output Capture

Add `"includeStdOut": true` to your job document:

```json
{
  "version": "1.0",
  "includeStdOut": true,
  "steps": [...]
}
```

Without this flag, you'll only see execution metadata (exit codes, timing) but not command output.

## Check Device Logs

For detailed debugging:

```bash
# Component logs
sudo tail -f /greengrass/v2/logs/com.example.DeviceOps.log

# Search for specific job
sudo grep "<job-id>" /greengrass/v2/logs/com.example.DeviceOps.log

# Greengrass service logs
sudo journalctl -u greengrass.service | grep "<job-id>"
```

## Important Notes

- **Size Limit**: AWS IoT Jobs statusDetails is limited to ~32KB
- **Multi-Step**: Uses compact JSON format to stay under AWS's 10 key-value pair limit
- **Security**: Avoid including sensitive data in stdout
- **Debugging**: Device logs always contain full output regardless of `includeStdOut` setting
