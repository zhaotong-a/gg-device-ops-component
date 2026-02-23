# Contributing to Device Operations Component

Thank you for your interest in contributing! This document provides guidelines for contributing to the project.

## Development Setup

### Prerequisites
- Docker (recommended) or Rust + zig + cargo-zigbuild
- AWS CLI configured
- Access to AWS IoT Core and Greengrass

### Quick Start
```bash
# Clone the repository
git clone https://github.com/YOUR_USERNAME/device-ops-component.git
cd device-ops-component

# Run tests
cargo test --lib

# Build
./scripts/docker-build.sh aarch64
```

## Testing

### Unit Tests
```bash
cargo test --lib
```

### E2E Tests
Requires a real device with Greengrass:
```bash
cd scripts/e2e-tests
./test-multi-step.sh <thing-name> simple
```

See [docs/TESTING_GUIDE.md](docs/TESTING_GUIDE.md) for details.

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix warnings
- Write unit tests for new features
- Update documentation

## Pull Request Process

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests (`cargo test --lib`)
5. Run formatting (`cargo fmt`)
6. Commit your changes (`git commit -m 'Add amazing feature'`)
7. Push to your fork (`git push origin feature/amazing-feature`)
8. Open a Pull Request

## Commit Messages

Use clear, descriptive commit messages:
- `feat: add multi-step job support`
- `fix: handle AWS IoT Jobs 10-field limit`
- `docs: update README with examples`
- `test: add unit tests for failure handling`

## Versioning

We use [Semantic Versioning](https://semver.org/):
- MAJOR: Breaking changes
- MINOR: New features (backward compatible)
- PATCH: Bug fixes

## Questions?

Open an issue or discussion on GitHub.
