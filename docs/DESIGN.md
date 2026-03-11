# Design: Greengrass Device Operations Component

---

## A. Executive Summary

**Problem Statement:**
Remote IoT edge devices need a way to execute commands and pre-installed scripts remotely — triggered from the cloud, with results reported back — without SSH access. The devices are resource-constrained (ARM64/x86_64, limited memory) and may be offline intermittently.

**Proposed Solution:**
A lightweight Greengrass component that receives job documents via AWS IoT Jobs, executes commands on the device (optionally pre-installed scripts), and reports results back to the cloud.

**Key Benefits:**
- Executes custom job templates with hardcoded commands (secure by design)
- Script path is optional — steps can run system binaries directly or pre-installed scripts
- Multi-step sequential execution with failure handling and cleanup
- IAM policies restrict which templates can be used (principle of least privilege)
- Works with Greengrass Nucleus Lite (resource-constrained devices)
- Supports both aarch64 and x86_64 Linux
- Offline device support — jobs queue until device reconnects, no custom infrastructure needed
- ~1 MB binary, <20 MB memory, <2s job latency

---

## B. Business Requirements

- Remotely execute commands or pre-installed scripts on edge devices without SSH access
- Target a single device or an entire fleet in one operation
- Return execution results (output, exit code, success/failure) to the operator
- Handle offline devices — queued operations execute automatically when the device reconnects
- Support multi-step operations: run several commands or scripts in sequence as a single unit of work
- Handle partial failures gracefully: skip optional steps, always run cleanup
- Secure by default: optional allowlist restricts which commands can be executed, with cloud-level access control
- Run on resource-constrained edge devices (ARM64 and x86_64)

---

## C. Architecture

### High-Level System Diagram

```
┌──────────────────────────────────────────────────┐
│  Cloud                                           │
│                                                  │
│  Job Templates ──► IoT Jobs Service              │
│                        │                         │
└────────────────────────┼─────────────────────────┘
                         │ MQTT/TLS
┌────────────────────────┼─────────────────────────┐
│  Device                │                         │
│                        ▼                         │
│  ┌─────────────────────────────┐                 │
│  │  Greengrass Nucleus (Lite)  │                 │
│  └──────────┬──────────────────┘                 │
│             │ IPC (gg-sdk)                       │
│  ┌──────────▼──────────────────┐                 │
│  │  Device Ops Component       │                 │
│  │                             │                 │
│  │  IPC Client (client.rs)     │                 │
│  │    ↓                        │                 │
│  │  Job Handler (jobs.rs)      │                 │
│  │    ↓                        │                 │
│  │  Validator (security.rs)    │                 │
│  │    ↓                        │                 │
│  │  Executor (executor.rs)     │                 │
│  └──────────┬──────────────────┘                 │
│             │ execvp (no shell)                  │
│  ┌──────────▼──────────────────┐                 │
│  │  System binaries / scripts  │                 │
│  │  (e.g. /sbin/ifconfig,     │                 │
│  │   /opt/device-scripts/*.sh) │                 │
│  └─────────────────────────────┘                 │
└──────────────────────────────────────────────────┘
```

### Key Components

1. **IPC Client** — Subscribes to MQTT topics via Greengrass IPC, publishes status updates, queries pending jobs
2. **Job Handler** — Event loop: receives jobs, validates, coordinates execution, reports status
3. **Security Validator** — Job document validation + optional allowlist for command/path restriction with symlink resolution
4. **Command Executor** — Spawns child processes with timeout enforcement, output capture, and process kill

### Job Execution Flow

```
Startup:
1. Subscribe to $next/get/accepted, publish $next/get
2. Process pending job (if any) to completion
3. Subscribe to notify-next → enter event loop

Per job:
4. Receive job via notify-next (or $next/get on startup)
5. Validate job document
6. Send IN_PROGRESS (starts IoT Jobs timeout timer)
7. For each step: security check → execute with timeout → evaluate success
8. Execute finalStep (always, regardless of prior failures)
9. Format statusDetails (≤10 fields, ≤1,024 chars/value)
10. Publish SUCCEEDED or FAILED
11. IoT Jobs auto-delivers next pending job via notify-next
```

### Job Delivery: Two-Phase Startup

**Phase 1: `$next/get` query** — On startup, subscribe to `$next/get/accepted` and publish to `$next/get`. This picks up any job queued while the device was offline. Process it to completion before moving on.

**Phase 2: `notify-next` subscription** — After the query phase completes, subscribe to `notify-next`. IoT Jobs pushes the next pending job whenever the queue head changes (new job created, current job completed). Handles steady-state while online.

By completing Phase 1 before subscribing in Phase 2, there is no overlap window where both mechanisms deliver the same job. No deduplication logic needed.

No custom IoT Core Rules, reconnection topics, or polling needed. Zero customer cloud setup beyond standard IoT Jobs.

---

## D. Why IoT Jobs + Custom Component

We evaluated five alternatives. The comparison (details in [Appendix A](#appendix-a-alternatives-analysis)):

| Criteria | IoT Commands | SSM | SSH | Custom MQTT | IoT Device Client | **This Component** |
|---|---|---|---|---|---|---|
| Memory | Custom agent | ~100 MB | N/A | Custom | ~50 MB | **<20 MB** |
| Cost (1000 devices, 1 op/month) | <$1/mo | ~$5,000/mo | $0 | $0 | $0 | **~$3/mo** |
| Offline job queuing | No | No | No | Custom | Yes | **Yes** |
| Fleet rollout control | No | Yes | No | Custom | Yes | **Yes** |
| Multi-step jobs | No | No | N/A | Custom | No | **Yes** |
| Greengrass Lite | Possible | No | N/A | Yes | No | **Yes** |
| Audit trail | Yes | Yes | No | Custom | Yes | **Yes** |
| Build effort | Medium | None | None | High | Low | **Medium** |

IoT Jobs gives us the fleet management primitives (queuing, targeting, rollout, audit) for free. The custom component adds the device-side execution layer that's small enough for constrained hardware and flexible enough for multi-step workflows with cleanup semantics.

---

## E. Security Model

### Defense in Depth

| Layer | Where | What |
|---|---|---|
| IAM policies | Cloud | Restrict which job templates can be used |
| Job templates | Cloud | Commands hardcoded, can't be changed at job creation |
| Document validation | Device | Version, action type, field constraints |
| Command allowlisting | Device (optional) | Single allowlist: exact match for commands, directory prefix for paths ending in `/` |
| Path traversal prevention | Device | Reject `..`, encoded variants in absolute paths |
| execvp (no shell) | Device | Arguments as array, no shell injection possible |
| File system permissions | Device | Scripts owned by root, read+execute only |

### Security Invariants

1. Commands are never passed through a shell — always direct execvp
2. Bare commands (e.g. `hostname`) resolve via PATH; absolute paths are used as-is
3. Path traversal checks and symlink resolution only apply to absolute paths
4. When the allowlist is non-empty, commands must match an entry (exact or directory prefix)
5. `runAsUser` requires verified passwordless sudo — falls back to current user on failure

### Example: IAM Policy Restricting to Specific Templates

```json
{
  "Effect": "Allow",
  "Action": "iot:CreateJob",
  "Resource": "arn:aws:iot:us-west-2:123456789012:job/*",
  "Condition": {
    "StringEquals": {
      "iot:JobTemplate": [
        "arn:aws:iot:us-west-2:123456789012:jobtemplate/get-store-id",
        "arn:aws:iot:us-west-2:123456789012:jobtemplate/get-camera-intrinsics"
      ]
    }
  }
}
```

---

## F. Use Cases

### Use Case 1: Get Store ID from DNS Hints

**Device:** Pre-installed script `/opt/device-scripts/get-store-id.sh` queries local DNS for store ID.

**Cloud:** Create job template with hardcoded command → Lambda creates job using template ARN → device executes and returns store ID in statusDetails.

### Use Case 2: Run Fleet Diagnostics

**Device:** Pre-installed script `/opt/device-scripts/run-diagnostics.sh` collects uptime, memory, disk, CPU temp.

**Cloud:** Create multi-step job targeting a device group → each device runs diagnostics and reports results → operator queries results via DescribeJobExecution.

### Use Case 3: Run a Direct System Command

**Device:** No pre-installed script needed. The job template references a system binary directly (e.g., `/sbin/ifconfig`).

**Cloud:** Create job template with `"command": "/sbin/ifconfig"` and `"args": ["eth0"]` → device executes and returns network interface info in statusDetails. Works when the allowlist is empty, or when the binary is in the allowlist.

### Use Case 4: Multi-Step with Cleanup

**Device:** Multiple scripts for data collection, with a cleanup step.

**Cloud:** Job template with 3 steps + finalStep. If step 2 fails, step 3 is skipped, but finalStep always runs to clean up temporary files.

---

## G. Implementation

### Component Structure

```
device-ops-component/
├── src/
│   ├── main.rs          # Entry point, logging, graceful shutdown
│   ├── config.rs         # JSON config loading with defaults
│   ├── error.rs          # Error types (thiserror)
│   ├── models.rs         # Job document structs, status formatting
│   ├── security.rs       # Document validation, allowlisting
│   ├── executor.rs       # Command execution, timeout, output capture
│   └── ipc/
│       ├── client.rs     # Greengrass IPC (subscribe, publish)
│       └── jobs.rs       # Job handler event loop
├── recipe.yaml           # Greengrass component recipe (aarch64 + x86_64)
├── config.json           # Default configuration
├── Cargo.toml            # Dependencies
└── Dockerfile            # Cross-compilation build environment
```

### Step Execution Semantics

1. Steps execute sequentially
2. Failure (exit code ≠ 0 or stderr > `allowStdErr`) stops the pipeline unless `ignoreStepFailure: true`
3. `finalStep` always runs (try/finally) — use for cleanup
4. Failed steps with `ignoreStepFailure` are recorded in output for observability
5. Overall status is SUCCEEDED only if all non-ignored steps succeeded

### Status Reporting

The component sends IN_PROGRESS before execution (starts the IoT Jobs `inProgressTimeoutInMinutes` timer), then SUCCEEDED or FAILED after completion.

**AWS IoT Jobs statusDetails constraints:**
- Max 10 key-value pairs
- Max 1,024 characters per value
- Max 128 characters per key
- All values must be strings

**Single-step:** Flat key-value pairs (step_name, exit_code, stdout, stderr).
**Multi-step:** Compact JSON array serialized as a single string value. Falls back to excluding stdout if over 1,024 chars.

### Configuration

```json
{
  "security": {
    "allowlist": [
      "/opt/device-scripts/",
      "hostname",
      "ifconfig"
    ]
  },
  "execution": {
    "defaultTimeout": 300
  }
}
```

The `allowlist` is a single list with simple rules:
- Entries ending in `/` are directory prefixes (e.g. `"/opt/device-scripts/"` allows anything under that directory)
- Everything else is an exact match (e.g. `"hostname"`, `"/usr/bin/uptime"`)
- Empty list (or omitted) = no restrictions, any command can run
- Non-empty list = only matching commands are permitted

### Error Handling

| Error | Response | Recovery |
|---|---|---|
| Malformed job document | Extract job ID if possible, report FAILED | IoT Jobs delivers next via notify-next |
| Security violation | Report FAILED before any step executes | Fix template, re-create job |
| Command timeout | Kill process group, report FAILED | Adjust timeout in template |
| Command not found | Record in step result, pipeline failure semantics | Fix device filesystem |
| IPC publish failure | Log error, job stays QUEUED | Component restart re-delivers via $next/get |

---

## H. What This Design Does NOT Cover

Intentionally out of scope for v1:

- **Job cancellation** — A running job runs to completion. No subscription to cancel signals.
- **Concurrent execution** — One job at a time. Queuing handled by IoT Jobs.
- **Script signature verification** — Scripts (when used) are trusted via filesystem permissions, not cryptographic signatures.
- **Persistent state** — No on-disk state. Acceptable because IoT Jobs handles re-delivery correctly (completed jobs are terminal, pending jobs are re-delivered on startup via `$next/get`).

These are candidates for future versions if the use case demands them.

---

## Appendix A: Alternatives Analysis

### AWS IoT Device Management Commands

AWS launched the [Commands feature](https://docs.aws.amazon.com/iot/latest/developerguide/iot-remote-command-concepts.html) (GA November 2024) for near-real-time remote actions on individual devices. Commands are sent as MQTT messages to reserved topics, and devices report status back.

**Pros:** Managed service, payload templates with parameter validation, concurrent execution, uses MQTT (works through Greengrass IPC).

**Why not:** Designed for single-device near-real-time actions, not fleet-scale batch operations. No job queuing for offline devices. No multi-step execution, no `ignoreStepFailure`, no `finalStep`. No fleet rollout control. Still requires a device-side agent to interpret payloads. Newer service (GA late 2024).

**When it's the right choice:** Interactive single-device operations where the device is known to be online — e.g., "turn on the LED on device X right now."

### AWS Systems Manager (SSM) Run Command

**Pros:** Fully managed, rich feature set (document parameters, S3 output, CloudWatch, rate control), mature.

**Why not:** SSM Agent requires ~100 MB memory (exceeds <20 MB budget). Advanced-instances tier costs ~$5/node/month ($5,000/month at 1000 devices). No Greengrass Nucleus Lite integration. Requires HTTPS, not MQTT.

### SSH / Direct Access

**Pros:** Simple, full interactive access.

**Why not:** Doesn't scale to 1000+ devices. Requires network reachability (NAT/firewall issues). No audit trail, no queuing, no offline support. Security risk.

### Custom MQTT Protocol (Without IoT Jobs)

**Pros:** Full protocol control, no IoT Jobs API constraints.

**Why not:** Reinvents queuing, status tracking, fleet targeting, retry logic, audit trail. High development and maintenance effort. No console UI.

### AWS IoT Device Client

**Pros:** Official AWS open-source, handles Jobs + Tunneling + Defender.

**Why not:** C++ (larger binary, harder to customize). Monolithic. No Greengrass Lite IPC integration. No multi-step, no finalStep, no ignoreStepFailure.

---

## Appendix B: AWS IoT Jobs API Facts

- `notify-next` fires on queue head change only, NOT on reconnection
- `$next/get` returns next pending job (IN_PROGRESS or QUEUED, IN_PROGRESS first)
- `UpdateJobExecution` status field: IN_PROGRESS, SUCCEEDED, FAILED, REJECTED
- statusDetails: max 10 pairs, max 1,024 chars/value, max 128 chars/key, strings only
- `inProgressTimeoutInMinutes` timer only starts when job enters IN_PROGRESS
- Direct QUEUED → SUCCEEDED/FAILED is valid but bypasses the timeout timer

---

## Appendix C: References

- AWS IoT Jobs: https://docs.aws.amazon.com/iot/latest/developerguide/iot-jobs.html
- IoT Jobs MQTT API: https://docs.aws.amazon.com/iot/latest/developerguide/jobs-mqtt-api.html
- IoT Jobs Lifecycle: https://docs.aws.amazon.com/iot/latest/developerguide/iot-jobs-lifecycle.html
- IoT Device Management Commands: https://docs.aws.amazon.com/iot/latest/developerguide/iot-remote-command-concepts.html
- IoT Device Management Pricing: https://aws.amazon.com/iot-device-management/pricing/
- Greengrass IPC: https://docs.aws.amazon.com/greengrass/v2/developerguide/interprocess-communication.html
- Greengrass Nucleus Lite: https://docs.aws.amazon.com/greengrass/v2/developerguide/greengrass-nucleus-lite-component.html
