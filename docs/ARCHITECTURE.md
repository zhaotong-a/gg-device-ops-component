# Architecture Documentation

## System Overview

The Device Operations Component is a lightweight Rust application that runs on AWS Greengrass devices to execute remote operations via AWS IoT Jobs.

## Component Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      Cloud Side                         │
│                                                         │
│  ┌──────────────┐         ┌──────────────┐            │
│  │  IoT Jobs    │◄────────┤ Job Templates│            │
│  │  Service     │         │  (Hardcoded  │            │
│  │              │         │   Commands)  │            │
│  └──────┬───────┘         └──────────────┘            │
│         │                                              │
└─────────┼──────────────────────────────────────────────┘
          │ MQTT over TLS
          │
┌─────────┼──────────────────────────────────────────────┐
│         │              Device Side                     │
│         ▼                                              │
│  ┌──────────────────────────────────────┐             │
│  │   Greengrass Nucleus                 │             │
│  │   - MQTT Client                      │             │
│  │   - IPC Server                       │             │
│  └──────────────┬───────────────────────┘             │
│                 │ IPC                                  │
│  ┌──────────────▼───────────────────────┐             │
│  │   Device Ops Component               │             │
│  │                                       │             │
│  │  ┌─────────────────────────────────┐ │             │
│  │  │  Job Handler                    │ │             │
│  │  │  - Subscribe to jobs            │ │             │
│  │  │  - Parse job documents          │ │             │
│  │  │  - Update job status            │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  │                │                      │             │
│  │  ┌─────────────▼───────────────────┐ │             │
│  │  │  Security Validator (Optional)  │ │             │
│  │  │  - Command allowlisting         │ │             │
│  │  │  - Path validation              │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  │                │                      │             │
│  │  ┌─────────────▼───────────────────┐ │             │
│  │  │  Command Executor               │ │             │
│  │  │  - Execute scripts              │ │             │
│  │  │  - Timeout handling             │ │             │
│  │  │  - Capture output               │ │             │
│  │  └─────────────┬───────────────────┘ │             │
│  └────────────────┼─────────────────────┘             │
│                   │                                    │
│  ┌────────────────▼─────────────────────┐             │
│  │   Pre-installed Device Scripts       │             │
│  │   /opt/device-scripts/               │             │
│  │   - get-store-id.sh                  │             │
│  │   - get-camera-intrinsics.sh         │             │
│  │   - run-diagnostics.sh               │             │
│  └──────────────────────────────────────┘             │
└─────────────────────────────────────────────────────────┘
```

## Module Structure

### Core Modules

#### 1. Main (`main.rs`)
- Entry point
- Initializes logging
- Loads configuration
- Creates IPC client
- Starts job handler
- Handles graceful shutdown

#### 2. Configuration (`config.rs`)
- Loads configuration from JSON file
- Provides default values
- Validates configuration
- Supports security and execution settings

#### 3. IPC Module (`ipc/`)

**IPC Client (`client.rs`)**
- Connects to Greengrass IPC socket
- Subscribes to IoT Jobs topics
- Publishes job status updates
- Requests pending jobs

**Job Handler (`jobs.rs`)**
- Main job processing loop
- Validates job documents
- Coordinates execution
- Updates job status
- Error handling

#### 4. Executor Module (`executor/`)

**Command Executor (`command.rs`)**
- Parses job documents
- Executes bash scripts
- Timeout handling
- Captures stdout/stderr
- Returns execution results

#### 5. Security Module (`security/`)

**Security Validator (`validation.rs`)**
- Command allowlisting
- Path allowlisting
- Path traversal prevention
- Job document validation
- Version checking
- Input sanitization

#### 6. Models Module (`models/`)

**Models (`models.rs`)**
- Job document structures
- Command structures
- Job status enums
- Execution output
- Serialization/deserialization

#### 7. Error Module (`error.rs`)
- Custom error types
- Error conversion
- Result type alias

## Data Flow

### Job Execution Flow

```
1. Cloud creates job using custom template
   ↓
2. IoT Jobs service sends notification via MQTT
   ↓
3. Greengrass Nucleus receives notification
   ↓
4. Greengrass forwards to Device Ops via IPC
   ↓
5. Job Handler receives job notification
   ↓
6. Validate job document structure
   ↓
7. Parse job document → extract command
   ↓
8. Security validation (if enabled)
   ↓
9. Execute script with timeout
   ↓
10. Capture stdout/stderr/exit code
   ↓
11. Update job status to SUCCEEDED/FAILED
   ↓
12. Request next pending job
```

**Note**: Jobs transition directly from QUEUED → SUCCEEDED/FAILED. No IN_PROGRESS status is sent to avoid AWS IoT Jobs rejecting updates with empty statusDetails.

### Error Handling Flow

```
Error Occurs
   ↓
Categorize Error Type
   ├─ IPC Error → Log + Retry
   ├─ Execution Error → Update job as FAILED
   ├─ Security Error → Update job as FAILED
   ├─ Timeout Error → Kill process + Update as FAILED
   └─ Config Error → Log + Use defaults
```

## Security Architecture

### Defense in Depth

**Layer 1: IAM Policies (Cloud)**
- Restrict which job templates can be used
- Prevent arbitrary command execution
- Audit trail via CloudTrail

**Layer 2: Job Templates (Cloud)**
- Commands hardcoded in templates
- Cannot be modified at job creation time
- Versioned and auditable

**Layer 3: Command Allowlisting (Device - Optional)**
- Validate script paths against allowlist
- Prevent path traversal
- Restrict to specific directories

**Layer 4: File System Permissions (Device)**
- Scripts owned by root
- Read-only for component user
- Execute-only permissions

### Security Model

```
┌─────────────────────────────────────────────────────┐
│  IAM Policy: Restrict to specific job templates    │
│  ✓ Prevents arbitrary commands                     │
│  ✓ Enforced by AWS                                 │
└─────────────────┬───────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────┐
│  Job Template: Hardcoded commands                  │
│  ✓ Commands cannot be changed at runtime           │
│  ✓ Versioned and auditable                         │
└─────────────────┬───────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────┐
│  Device Allowlist: Optional validation (Optional)  │
│  ✓ Defense in depth                                │
│  ✓ Prevents misconfiguration                       │
└─────────────────┬───────────────────────────────────┘
                  │
┌─────────────────▼───────────────────────────────────┐
│  File System: Permissions and ownership            │
│  ✓ Scripts read-only                               │
│  ✓ Prevents tampering                              │
└─────────────────────────────────────────────────────┘
```

## Performance Characteristics

### Resource Usage

- **Binary Size**: ~1.1MB (release build with LTO and strip)
- **Memory Usage**: 
  - Idle: ~10-20MB
  - Active: ~10-20MB
- **CPU Usage**:
  - Idle: <1%
  - During execution: <5%
- **Startup Time**: <5 seconds
- **Job Latency**: <2s from notification to execution

### Scalability

- **Job Queue**: Unlimited (managed by IoT Jobs)
- **Timeout**: Configurable per job (default: 300 seconds / 5 minutes)
- **Result Size**: Limited by IoT Jobs (32KB statusDetails)
- **Status Updates**: Direct QUEUED → SUCCEEDED/FAILED (no intermediate IN_PROGRESS)

## Deployment Architecture

### Single Device

```
Device
├── Greengrass Nucleus
├── Device Ops Component
└── Device Scripts
```

### Fleet Deployment

```
IoT Jobs
├── Job Template 1 → Device Group A (100 devices)
├── Job Template 2 → Device Group B (500 devices)
└── Job Template 3 → All Devices (1000 devices)
```

### Multi-Region

```
Region 1 (us-west-2)
├── IoT Jobs Service
├── Device Fleet A
└── Component Artifacts (S3)

Region 2 (eu-west-1)
├── IoT Jobs Service
├── Device Fleet B
└── Component Artifacts (S3)
```

## Monitoring & Observability

### Logging

- **Structured Logging**: JSON format with tracing
- **Log Levels**: ERROR, WARN, INFO, DEBUG, TRACE
- **Log Destination**: 
  - Local: `/greengrass/v2/logs/com.example.DeviceOps.log`
  - CloudWatch: Optional via Greengrass log manager

### Metrics

- Job execution count
- Job success/failure rate
- Execution duration
- Timeout occurrences
- Security validation failures

### Tracing

- Job ID correlation
- Execution timeline
- Error context
- Performance profiling

## Failure Modes & Recovery

### Component Crash
- **Detection**: Greengrass monitors process
- **Recovery**: Automatic restart by Greengrass
- **State**: Jobs resume from queue

### Network Disconnection
- **Detection**: MQTT connection loss
- **Recovery**: Automatic reconnection
- **State**: Jobs queue in cloud until reconnection

### Script Failure
- **Detection**: Non-zero exit code
- **Recovery**: Job marked as FAILED
- **State**: Error details in job status

### Timeout
- **Detection**: Execution exceeds timeout
- **Recovery**: Process killed, job marked as FAILED
- **State**: Timeout error in job status

## Future Enhancements

1. **Long-running Operations**
   - Progress updates
   - Cancellation support
   - Streaming results

2. **Advanced Features**
   - Job prioritization
   - Rate limiting

3. **Observability**
   - CloudWatch metrics export
   - Distributed tracing
   - Performance profiling

4. **Security**
   - Script signature verification
   - Encrypted job documents
   - Audit logging
