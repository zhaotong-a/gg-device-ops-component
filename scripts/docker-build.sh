#!/bin/bash
# Build and test inside Docker container
# Usage: ./scripts/docker-build.sh [x86_64|aarch64|all]

set -e

# Get the project root directory (parent of scripts/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Change to project root
cd "$PROJECT_ROOT"

# Use default builder (not buildx)
export DOCKER_BUILDKIT=0

# Determine target architecture
TARGET="${1:-x86_64}"

# Build the Docker image
echo "Building Docker image..."
docker build -t device-ops-builder .

if [ "$TARGET" = "all" ]; then
    echo ""
    echo "Building for all architectures..."
    
    # Build x86_64
    echo ""
    echo "=== Building x86_64 ==="
    docker run --rm -v "$(pwd):/workspace" device-ops-builder bash -c "
        set -e
        
        echo '=== Running library tests (x86_64) ==='
        CC='zig cc' CFLAGS='-target x86_64-linux-gnu.2.31' cargo test --lib --target x86_64-unknown-linux-gnu
        
        echo ''
        echo '=== Running integration tests (x86_64) ==='
        CC='zig cc' CFLAGS='-target x86_64-linux-gnu.2.31' cargo test --test integration_test --target x86_64-unknown-linux-gnu
        
        echo ''
        echo '=== Building release binary (x86_64) ==='
        cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31
        
        echo ''
        echo '✅ x86_64 build complete!'
        ls -lh target/x86_64-unknown-linux-gnu/release/device-ops-component
    "
    
    # Build aarch64
    echo ""
    echo "=== Building aarch64 ==="
    docker run --rm -v "$(pwd):/workspace" device-ops-builder bash -c "
        set -e
        
        echo '=== Installing aarch64 target ==='
        rustup target add aarch64-unknown-linux-gnu
        
        echo ''
        echo '=== Building release binary (aarch64) ==='
        cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.31
        
        echo ''
        echo '✅ aarch64 build complete!'
        ls -lh target/aarch64-unknown-linux-gnu/release/device-ops-component
    "
    
    echo ""
    echo "✅ All architectures built successfully!"
    
elif [ "$TARGET" = "aarch64" ]; then
    echo ""
    echo "Building for aarch64 only..."
    docker run --rm -v "$(pwd):/workspace" device-ops-builder bash -c "
        set -e
        
        echo '=== Installing aarch64 target ==='
        rustup target add aarch64-unknown-linux-gnu
        
        echo ''
        echo '=== Building release binary (aarch64) ==='
        cargo zigbuild --release --target aarch64-unknown-linux-gnu.2.31
        
        echo ''
        echo '✅ aarch64 build complete!'
        echo 'Binary location: target/aarch64-unknown-linux-gnu/release/device-ops-component'
        ls -lh target/aarch64-unknown-linux-gnu/release/device-ops-component
    "
    
else
    # Default: x86_64
    echo ""
    echo "Building for x86_64..."
    docker run --rm -v "$(pwd):/workspace" device-ops-builder bash -c "
        set -e
        
        echo '=== Running library tests ==='
        CC='zig cc' CFLAGS='-target x86_64-linux-gnu.2.31' cargo test --lib --target x86_64-unknown-linux-gnu
        
        echo ''
        echo '=== Running integration tests ==='
        CC='zig cc' CFLAGS='-target x86_64-linux-gnu.2.31' cargo test --test integration_test --target x86_64-unknown-linux-gnu
        
        echo ''
        echo '=== Building release binary ==='
        cargo zigbuild --release --target x86_64-unknown-linux-gnu.2.31
        
        echo ''
        echo '✅ All tests passed and binary built successfully!'
        echo 'Binary location: target/x86_64-unknown-linux-gnu/release/device-ops-component'
        ls -lh target/x86_64-unknown-linux-gnu/release/device-ops-component
    "
fi
