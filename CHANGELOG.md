# Changelog

All notable changes to the Device Operations Component will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.0.0] - 2026-02-23

### Initial Release

First public release of the Device Operations Component for AWS IoT Greengrass.

**Features:**
- Execute pre-installed bash scripts on devices via AWS IoT Jobs
- Multi-step jobs with sequential execution and failure handling
- Final step execution for cleanup/summary tasks
- Automatic reconnection detection and job recovery
- IAM-based security with job template restrictions
- Optional command allowlisting for defense-in-depth
- Compatible with Greengrass Nucleus Lite
- ~1.1MB binary, <20MB memory, <2s job latency

**Architecture:**
- Trait-based dependency injection for testability
- Comprehensive unit test coverage with mocks
- Compact status format for AWS IoT Jobs 10-field limit
- VecDeque-based job deduplication (FIFO)

**Documentation:**
- Complete deployment guide with IoT Core Rule setup
- Multi-step job examples and templates
- E2E test suite for real device validation
- Architecture and testing guides
