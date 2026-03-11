# Architecture Documentation

## System Overview

The Device Operations Component is a lightweight Rust application that runs on AWS Greengrass devices to execute remote operations via AWS IoT Jobs. It receives job documents via MQTT (through Greengrass IPC), validates them, optionally checks security allowlists, executes commands (system binaries or pre-installed scripts) with timeout enforcement, and reports results back to the cloud.

## Component Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      Cloud Side                         │
│                                                         │
│  ┌──────────────┐         ┌──────────────┐            │
│  │  IoT Jobs    │◄────────┤ Job Templates│            │
│  │  Service     │         │  (Hardcoded  │            │
│  │  (notify-    │         │   Commands)  │            │
│  │   next)      │         └──────────────┘            │
│  └──────┬───────┘                                     │
│         │                                              │
└─────────┼──────────────────────────────────────────────┘
          │ MQTT over TLS
          │
┌─────────┼──────────────────────────────────────────────┐
│         │              Device Side                     │
│         ▼                                              │
│  ┌──────────────────────────────────────┐             │
│  │   Greengrass Nucleus (Lite)          │             │
│  │   - MQTT Client                      │             │
│  │   - IPC Server                       │             │
│  └──────────────┬───────────────────────┘             │
│                 │ IPC (gg-sdk)                         │
│  ┌──────────────▼───────────────────────┐             │
│  │   Device Ops Component               │             │
│  │                                       │             │
│  │  ┌─────────────────────────────────┐ │             │
│  │  │  IPC Client (client.rs)         │ │             │
│  │  │  - Subscribe to MQTT topics     │ │             │
│  │  │  - Publish status updates       │ │             │
│  │  │  - Request pending jobs         │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  │                │                      │             │
│  │  ┌─────────────▼───────────────────┐ │             │
│  │  │  Job Handler (jobs.rs)          │ │             │
│  │  │  - Validate job documents       │ │             │
│  │  │  - Coordinate execution         │ │             │
│  │  │  - Report status                │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  │                │                      │             │
│  │  ┌─────────────▼───────────────────┐ │             │
│  │  │  Security Validator (Optional)  │ │             │
│  │  │  - Allowlist enforcement        │ │             │
│  │  │  - Path traversal prevention    │ │             │
│  │  │  - Symlink resolution           │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  │                │                      │             │
│  │  ┌─────────────▼───────────────────┐ │             │
│  │  │  Command Executor (executor.rs)  │ │             │
│  │  │  - Build command (sudo support) │ │             │
│  │  │  - Execute with timeout         │ │             │
│  │  │  - Kill process on timeout      │ │             │
│  │  │  - Capture & truncate output    │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  └────────────────┼─────────────────────┘             │
│                   │                                    │
│  ┌────────────────▼─────────────────────┐             │
│  │   System Binaries / Device Scripts  │             │
│  │   /opt/device-scripts/ (optional)   │             │
│  │   /sbin/ifconfig, /bin/hostname...  │             │
│  └──────────────────────────────────────┘             │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

### Core Modules

#### 1. Main (`main.rs`)
- Entry point with `#[tokio::main]`
- Initializes structured logging via `tracing_subscriber` with `EnvFilter`
- Loads configuration from JSON (falls back to defaults if file missing)
- Creates IPC client (connects to Greengrass SDK)
- Creates JobHandler, starts event loop
- Graceful shutdown via `tokio::select!` on `ctrl_c`

#### 2. Configuration (`config.rs`)
- Loads from `/greengrass/v2/config/device-ops-config.json` (hardcoded default path)
- Accepts optional override path for testing
- Falls back to `Config::default()` if file not found
- Structs: `Config`, `SecurityConfig`, `ExecutionConfig`
- **Requirement**: JSON field names use camelCase (`allowlist`); serde deserialization must handle the rename correctly
- **Requirement**: Config path should be overridable via environment variable for operational flexibility

#### 3. IPC Client (`ipc/client.rs`)
- Wraps `gg_sdk::Sdk` for Greengrass IPC communication
- Resolves thing name from `AWS_IOT_THING_NAME` env var (must fail hard if unavailable)
- Subscribes to MQTT topics via IPC:
  - `$aws/things/{thing}/jobs/notify-next` — IoT Jobs pushes the next pending job when the head of the queue changes (new job created, current job completed)
  - `$aws/things/{thing}/jobs/$next/get/accepted` — response to explicit `$next/get` queries
  - `$aws/things/{thing}/jobs/+/update/accepted` — debug: AWS accepted status update
  - `$aws/things/{thing}/jobs/+/update/rejected` — debug: AWS rejected status update
- Publishes to:
  - `$aws/things/{thing}/jobs/{id}/update` — report job status (IN_PROGRESS, SUCCEEDED, FAILED)
  - `$aws/things/{thing}/jobs/$next/get` — explicitly query for next pending job (used on startup)
- Uses `Box::leak` for callback lifetime (intentional, program-lifetime callbacks)
- Uses `mpsc::channel` to bridge sync callbacks into async event loop
- **Requirement**: Callbacks must use non-blocking channel sends (`try_send`) since the Greengrass SDK may invoke callbacks from contexts where blocking is unsafe

#### 4. Job Handler (`ipc/jobs.rs`)
- Owns `IpcClient` and `CommandExecutor`
- Simple event loop: receives jobs from channel, processes sequentially
- Two-phase startup eliminates duplicate job delivery:
  1. Query `$next/get` to pick up any job queued while offline, process to completion
  2. Subscribe to `notify-next` for steady-state job delivery
- By completing Phase 1 before subscribing in Phase 2, there is no overlap window
- Handles both valid jobs and parse errors (malformed job documents)
- Sends IN_PROGRESS before executing steps — this starts the IoT Jobs `inProgressTimeoutInMinutes` timer, which auto-fails the job if the device dies mid-execution
- Reports terminal status (SUCCEEDED/FAILED) after all steps complete
- No custom IoT Core Rules, reconnection topics, or polling needed

#### 5. Command Executor (`executor.rs`)
- Generic over `CommandRunner` trait for testability
- `SystemCommandRunner`: real implementation using `tokio::process::Command`
- `execute()`: runs all steps sequentially, handles `ignoreStepFailure`, runs `finalStep` always (like `try/finally` — for cleanup regardless of success/failure)
- `execute_step()`: builds command, runs security validation, executes with timeout
- `build_command()`: resolves `runAsUser` via sudo verification
- **Requirement**: Timeout must actually kill the child process, not just drop the future (orphaned processes are a resource leak on constrained edge devices)
- **Requirement**: `verify_sudo_and_user()` must not use blocking `std::process::Command` inside the async runtime — must use `tokio::process::Command` or `spawn_blocking`
- **Requirement**: When a step errors with `ignoreStepFailure=true`, a `StepOutput` record must still be emitted for observability
- Output truncation: limits to 1000 lines and 1,024 characters per field (AWS IoT Jobs statusDetails value limit)
- `evaluate_step_success()`: checks exit code and stderr line count against `allowStdErr`

#### 6. Security Validator (`security.rs`)
- `validate_job_document()`: validates version ("1.0"), non-empty steps, action type ("runCommand"), command length (≤4096), non-empty command, timeout range (1–86400)
- `SecurityValidator`: runtime allowlist checking (created only when allowlist is non-empty)
  - Single allowlist: entries ending in `/` are directory prefixes, others are exact match
  - For absolute paths: path traversal detection, symlink resolution via `canonicalize`, then allowlist check
  - For bare commands (e.g. `hostname`): exact match against allowlist only
- **Requirement**: Symlinks must be resolved before path validation (`std::fs::canonicalize`) to prevent symlink-based bypasses
- **Requirement**: Path traversal checks only apply to absolute paths; bare commands skip path validation
- **Requirement**: Document that args are passed via `execvp` (no shell), so shell injection via args is not possible — but this must be explicitly maintained as an invariant

#### 7. Models (`models.rs`)
- Job document structures with serde rename for camelCase JSON fields
- `JobNotification` → `JobExecution` → `Job` conversion
- `JobOrError` enum for handling malformed notifications gracefully
- `ExecutionOutput`, `Command`, `JobExecutionResult`, `StepOutput`
- `JobStatus` with `to_json()` for IoT Jobs API
- `format_status_details()`: formats results for IoT Jobs statusDetails (10 field limit, all string values)
  - Single step: individual fields
  - Multi-step: compact JSON array serialized as string

#### 8. Error Module (`error.rs`)
- `DeviceOpsError` enum via `thiserror`: IpcError, ExecutionError, SecurityError, ConfigError, TimeoutError, InvalidJobDocument
- `Result<T>` type alias

## Data Flow

### Job Execution Flow

```
1. Cloud creates job targeting device (via job template)
   ↓
2. IoT Jobs service sends notification via MQTT to notify-next
   ↓
3. Greengrass Nucleus receives MQTT message
   ↓
4. Greengrass forwards to Device Ops via IPC subscription callback
   ↓
5. Callback parses payload, sends to async channel
   ↓
6. Job Handler receives from channel, validates job document
   ↓
7. Validate job document (version, steps, action types, timeouts)
   ↓
8. Send IN_PROGRESS status (starts IoT Jobs timeout timer)
   ↓
9. For each step:
   a. Build command (resolve runAsUser/sudo)
   b. Security validation (if allowlist non-empty): path traversal, allowlist check
   c. Execute with timeout (kill on timeout)
   d. Evaluate success (exit code + stderr threshold)
   e. If failed and !ignoreStepFailure → stop
   ↓
10. Execute finalStep if present (always runs, like try/finally — for cleanup)
   ↓
11. Format status details (≤10 fields, ≤1,024 chars per value)
   ↓
12. Publish SUCCEEDED/FAILED to $aws/things/{thing}/jobs/{id}/update
   ↓
13. IoT Jobs automatically delivers next pending job via notify-next
```

### Reconnection Handling

No custom IoT Core Rules or reconnection topics are needed. Two-phase startup covers job delivery:

1. **`$next/get` query (Phase 1)** — On startup, queries for any pending job queued while offline. Processed to completion before Phase 2.

2. **`notify-next` subscription (Phase 2)** — After Phase 1 completes, subscribes for steady-state delivery. IoT Jobs pushes the next pending job whenever the queue head changes.

By completing Phase 1 before subscribing in Phase 2, there is no overlap window where both mechanisms deliver the same job. No deduplication logic needed.

Together these ensure no jobs are missed under normal operation. The customer sets up zero cloud infrastructure beyond the standard IoT Jobs setup.

**Known edge case**: If the network is down longer than the MQTT persistent session duration (~1 hour on AWS IoT Core), the Greengrass Nucleus reconnects with a clean session. If the component process stays alive through this (no restart), and no new jobs are created afterward, a queued job may sit unprocessed until the next component restart or new job creation. This is an extremely narrow scenario — the job is not lost (it remains QUEUED in IoT Jobs), and any of these events will unstick it: component restart, new job targeting the device, or manual query.

### Error Handling Flow

```
Error Occurs
   ↓
Categorize Error Type
   ├─ Parse Error → Extract job ID from raw JSON → FAILED with parse error message
   ├─ Validation Error → FAILED with validation message
   ├─ Security Error → FAILED with security message (step stops)
   ├─ Execution Error (ignoreStepFailure=false) → FAILED with error → stop pipeline
   ├─ Execution Error (ignoreStepFailure=true) → Log warning → continue pipeline
   ├─ Timeout Error → Kill child process → FAILED with timeout message
   ├─ IPC Error → Log + propagate (may cause restart)
   └─ Config Error → Log warning + use defaults
```

In all cases where a job is marked FAILED, IoT Jobs removes it from the queue and `notify-next` automatically delivers the next pending job if one exists.

## Security Architecture

### Defense in Depth

**Layer 1: IAM Policies (Cloud)**
- Restrict which job templates can be used via IAM conditions
- Prevent arbitrary command execution
- Audit trail via CloudTrail

**Layer 2: Job Templates (Cloud)**
- Commands hardcoded in templates
- Cannot be modified at job creation time
- Versioned and auditable

**Layer 3: Job Document Validation (Device)**
- Version check (only "1.0" accepted)
- Action type check (only "runCommand" accepted)
- Command length limit (4096 chars)
- Empty command rejection
- Timeout range validation (1–86400 seconds)

**Layer 4: Command Allowlisting (Device — Optional)**
- Single allowlist: entries ending in `/` are directory prefixes, others are exact match
- Empty allowlist = no restrictions; non-empty = enforced
- Path traversal prevention (`..`, encoded variants) for absolute paths only
- Symlink resolution before validation (absolute paths only)

**Layer 5: Process Execution (Device)**
- No shell invocation — commands executed via `execvp` (no shell injection via args)
- Timeout enforcement with process kill
- Output truncation to prevent memory exhaustion
- Optional `runAsUser` via passwordless sudo (verified before use)

**Layer 6: File System Permissions (Device)**
- Scripts owned by root
- Read-only for component user
- Execute-only permissions

### Security Invariants

1. Commands are never passed through a shell — always direct `execvp`
2. All command paths must be absolute (for script paths; bare commands resolve via PATH)
3. Path traversal patterns are rejected before execution (absolute paths only)
4. When the allowlist is non-empty, commands are checked against it (exact match or directory prefix)
5. Symlinks are resolved to real paths before allowlist checking
6. `runAsUser` requires verified passwordless sudo — falls back to current user on failure

## Performance Characteristics

### Resource Usage

- **Binary Size**: ~1.1MB (release build with LTO, strip, panic=abort)
- **Memory Usage**: ~10–20MB (idle and active)
- **CPU Usage**: <1% idle, <5% during execution
- **Startup Time**: <5 seconds
- **Job Latency**: <2s from notification to execution start

### Output Limits

- **Max output lines**: 1000 per stream (stdout/stderr)
- **Max output chars per field**: 1,024 (AWS IoT Jobs statusDetails value limit)
- **Status details fields**: ≤10 key-value pairs (AWS IoT Jobs limit)
- **Status details key length**: ≤128 characters
- **Command length**: ≤4096 characters
- **Timeout range**: 1–86400 seconds (24 hours)

### Scalability

- **Concurrency**: Single job at a time (sequential processing)
- **Job Queue**: Unlimited (managed by IoT Jobs cloud service)
- **Channel Buffer**: 100 messages (job notifications)

## Deployment Architecture

### Single Device

```
Device (aarch64 or x86_64)
├── Greengrass Nucleus (Lite)
├── Device Ops Component (platform-specific binary)
├── Config: /greengrass/v2/config/device-ops-config.json
├── Logs: /greengrass/v2/logs/com.example.DeviceOps.log
└── Device Scripts: /opt/device-scripts/
```

### Fleet Deployment

```
IoT Jobs
├── Job Template 1 → Device Group A (100 devices)
├── Job Template 2 → Device Group B (500 devices)
└── Job Template 3 → All Devices (1000 devices)
```

### Build & Cross-Compilation

- Docker-based build environment (Ubuntu 22.04)
- Rust + Zig + cargo-zigbuild for cross-compilation
- Supported targets: aarch64-unknown-linux-gnu, x86_64-unknown-linux-gnu
- Release profile: opt-level="z", LTO, codegen-units=1, strip, panic=abort

## Monitoring & Observability

### Logging

- **Framework**: `tracing` + `tracing-subscriber` with `EnvFilter`
- **Format**: Structured key-value fields (not JSON by default — `fmt::layer()` uses human-readable format)
- **Log Levels**: Controlled via `RUST_LOG` env var (default: `device_ops_component=info`)
- **Log Destination**:
  - Local: `/greengrass/v2/logs/com.example.DeviceOps.log`
  - CloudWatch: Optional via Greengrass log manager component
- **Key fields logged**: job_id, step_name, exit_code, execution_time_ms, error, topic

### Operational Signals

- Job received / completed / failed (INFO/ERROR)
- Step execution start / success / failure (INFO/WARN/ERROR)
- AWS accepted/rejected status updates (INFO/ERROR)
- Security validation failures (ERROR)
- Timeout events (ERROR)
- Channel send failures (ERROR)

## Failure Modes & Recovery

### Component Crash
- **Detection**: Greengrass monitors component process
- **Recovery**: Automatic restart by Greengrass Nucleus
- **State**: On restart, queries pending jobs via `$next/get`

### Network Disconnection
- **Detection**: MQTT connection loss (handled by Greengrass Nucleus)
- **Recovery**: On component restart, the startup `$next/get` query picks up pending jobs. For short disconnections (< session expiry), MQTT persistent session redelivers queued messages automatically.
- **State**: Jobs queue in cloud until device reconnects — no custom infrastructure needed
- **Known limitation**: If the component process stays alive across a clean-session reconnect with no new jobs created, a queued job may wait until the next restart or new job. See Reconnection Handling.

### Script Failure
- **Detection**: Non-zero exit code or stderr exceeding `allowStdErr` threshold
- **Recovery**: Job marked as FAILED with details
- **State**: Error details in statusDetails (exit code, stderr, step name)

### Timeout
- **Detection**: `tokio::time::timeout` fires
- **Recovery**: Child process killed, job marked as FAILED
- **State**: TimeoutError with duration in statusDetails

### Malformed Job Document
- **Detection**: serde deserialization failure or validation failure
- **Recovery**: Job ID extracted from raw JSON if possible, marked as FAILED
- **State**: Parse/validation error message in statusDetails

### IPC Connection Failure
- **Detection**: gg-sdk connect/subscribe/publish errors
- **Recovery**: Error propagated, component may restart via Greengrass
- **State**: Logged with full error context

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| tokio | 1.35 | Async runtime (full features) |
| serde / serde_json | 1.0 | Serialization/deserialization |
| tracing / tracing-subscriber | 0.1 / 0.3 | Structured logging |
| thiserror | 1.0 | Error type derivation |
| async-trait | 0.1 | Async trait support |
| gg-sdk | git (main) | Greengrass IPC SDK |
| mockall | 0.12 | Test mocking (dev) |
| tempfile | 3.8 | Temp files for tests (dev) |

## Future Enhancements

1. **Process Management**
   - Job cancellation support (subscribe to `$aws/things/{thing}/jobs/{id}/update` for cancel signals)
   - Periodic IN_PROGRESS updates with step-level progress for long-running jobs
   - Consider migrating from `$next/get` to `start-next` MQTT API for atomic get+start

2. **Reliability**
   - Retry logic for transient IPC publish failures
   - Rate limiting for job execution

3. **Observability**
   - CloudWatch metrics export (job count, latency, failure rate)
   - Structured JSON log format option

4. **Security**
   - Script signature verification (hash or GPG)
   - Audit logging to separate file/stream
   - Argument validation/sanitization rules
