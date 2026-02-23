# Build and test environment with glibc 2.31+
FROM ubuntu:22.04

# Avoid interactive prompts
ENV DEBIAN_FRONTEND=noninteractive

# Install build dependencies
RUN apt-get update && apt-get install -y \
    curl \
    build-essential \
    pkg-config \
    libssl-dev \
    git \
    wget \
    xz-utils \
    clang \
    libclang-dev \
    llvm-dev \
    libc6-dev \
    linux-libc-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Install Zig (detect architecture)
RUN cd /tmp && \
    ARCH=$(uname -m) && \
    if [ "$ARCH" = "x86_64" ]; then \
        ZIG_ARCH="x86_64"; \
    elif [ "$ARCH" = "aarch64" ]; then \
        ZIG_ARCH="aarch64"; \
    else \
        echo "Unsupported architecture: $ARCH" && exit 1; \
    fi && \
    wget https://ziglang.org/download/0.13.0/zig-linux-${ZIG_ARCH}-0.13.0.tar.xz && \
    tar -xf zig-linux-${ZIG_ARCH}-0.13.0.tar.xz && \
    mv zig-linux-${ZIG_ARCH}-0.13.0 /usr/local/zig && \
    rm zig-linux-${ZIG_ARCH}-0.13.0.tar.xz
ENV PATH="/usr/local/zig:${PATH}"

# Install cargo-zigbuild
RUN cargo install cargo-zigbuild

# Set working directory
WORKDIR /workspace

# Default command
CMD ["/bin/bash"]
