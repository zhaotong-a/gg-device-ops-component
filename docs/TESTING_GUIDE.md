# Testing Guide

This document explains the testing structure and how to run tests.

## Test Structure

### 1. Unit Tests (Automated)

**Location**: `src/**/*.rs` (in `#[cfg(test)]` modules)

**What they test**:
- Command execution logic with mocks
- Multi-step job execution
- Failure handling (`ignoreStepFailure`, `allowStdErr`)
- Final step logic
- Status formatting
- Security validation

**Run with**:
```bash
# All unit tests
cargo test --lib

# Specific test
cargo test test_multi_step_execution_logic

# In Docker (recommended)
./scripts/docker-build.sh aarch64
```

**Coverage**: ~20 tests covering all core logic

### 2. End-to-End Tests (Manual)

**Location**: `scripts/e2e-tests/`

**What they test**:
- Real device deployment
- AWS IoT Jobs integration
- Greengrass IPC communication
- Multi-step job execution on devices

**Run with**:
```bash
cd scripts/e2e-tests

# Simple multi-step test (basic commands)
./test-multi-step.sh <thing-name> simple

# Failure handling test
./test-multi-step.sh <thing-name> failure

# Device scripts test (requires scripts installed)
./test-multi-step.sh <thing-name> device
```

See [scripts/e2e-tests/README.md](../scripts/e2e-tests/README.md) for details.

## Quick Start

### Run Unit Tests Locally
```bash
cargo test --lib
```

### Run Unit Tests in Docker
```bash
./scripts/docker-build.sh aarch64
```

### Run E2E Tests on Device
```bash
# 1. Deploy component
./scripts/deploy-to-device.sh my-device 1.0.0

# 2. Run test
cd scripts/e2e-tests
./test-multi-step.sh my-device simple

# 3. Check results
aws iot describe-job-execution \
  --job-id <job-id> \
  --thing-name my-device \
  --query 'execution.statusDetails.detailsMap.steps' \
  --output text | jq .
```

## Test Architecture

### Trait-Based Mocking

The code uses traits for testability:

```rust
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, command: &Command) -> Result<ExecutionOutput>;
}
```

**Implementations**:
- `SystemCommandRunner` - Production (executes real commands)
- `MockCommandRunner` - Testing (returns predefined responses)

**Usage in tests**:
```rust
let mock = MockCommandRunner::new(vec![
    Ok(ExecutionOutput { stdout: "test", exit_code: 0, ... }),
]);
let executor = CommandExecutor::new_with_runner(config, None, mock);
```

## CI/CD Integration

### GitHub Actions Example
```yaml
- name: Run tests
  run: cargo test --lib
```

### Docker Build
```bash
./scripts/docker-build.sh aarch64  # Runs tests automatically
```

## Adding New Tests

### Unit Test
Add to `src/**/*.rs` in `#[cfg(test)]` module:
```rust
#[tokio::test]
async fn test_my_feature() {
    let mock = MockCommandRunner::new(vec![...]);
    let executor = CommandExecutor::new_with_runner(config, None, mock);
    // Test logic
}
```

### E2E Test
Add script to `scripts/e2e-tests/` or job template to `job-templates/`.

## Test Coverage

| Type | Count | Speed | Dependencies |
|------|-------|-------|--------------|
| Unit Tests | ~20 | Fast | None (mocked) |
| E2E Tests | 3 scenarios | Slow | Real device + AWS |

## Benefits

- ✅ Fast unit tests with no external dependencies
- ✅ Comprehensive logic coverage with mocks
- ✅ Real-world validation with E2E tests
- ✅ Clear separation of concerns
- ✅ CI/CD friendly
