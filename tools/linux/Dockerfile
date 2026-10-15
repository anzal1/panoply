# The Linux check image for tools/preflight-linux.sh: what CI's ubuntu-latest job has, in one image.
#
#   * Ubuntu 24.04 (the same release as ubuntu-latest), native arm64 under colima, so it runs at near
#     host speed. CI is x86_64; the code paths are the same, only the instruction set differs.
#   * The Rust toolchain from rust-toolchain.toml (rustfmt, clippy) plus the wasm32-wasip2 target the
#     WebAssembly tests need, and cargo-nextest.
#   * The apt packages of .github/actions/linux-libs, which the GPUI desktop app needs to compile.
#     Keep this list in step with that file (tools/preflight-linux.sh --check-libs compares them).
#   * xvfb, openbox, xdotool and the X tools for the headless desktop smoke (--smoke).
#
# The build context is a folder holding this file and rust-toolchain.toml; preflight-linux.sh makes it.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    CARGO_TERM_COLOR=always \
    CARGO_INCREMENTAL=0 \
    CARGO_NET_RETRY=10 \
    RUST_BACKTRACE=short

# BEGIN linux-libs (copied from .github/actions/linux-libs/action.yml)
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      pkg-config cmake clang libclang-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
      libx11-dev libx11-xcb-dev libxcb1-dev libxcb-xkb-dev libxcb-render0-dev libxcb-shape0-dev \
      libxcb-xfixes0-dev libfontconfig-dev libfreetype-dev libssl-dev libasound2-dev libvulkan-dev \
      libdbus-1-dev libgtk-3-dev \
 && rm -rf /var/lib/apt/lists/*
# END linux-libs

# What the runner image already has (a C toolchain, git, curl, python) and the headless desktop.
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      build-essential ca-certificates curl git rsync python3 procps \
      xvfb openbox xdotool wmctrl x11-apps x11-utils dbus-x11 imagemagick \
      mesa-vulkan-drivers vulkan-tools fonts-dejavu-core \
 && rm -rf /var/lib/apt/lists/*

# The toolchain comes from rust-toolchain.toml itself (channel and components), so a change there
# changes the image (the image tag hashes that file).
COPY rust-toolchain.toml /tmp/tc/rust-toolchain.toml
WORKDIR /tmp/tc
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none \
 && rustup toolchain install \
 && rustup target add wasm32-wasip2 \
 && curl -LsSf https://get.nexte.st/latest/linux-arm | tar zxf - -C /usr/local/cargo/bin \
 && cargo nextest --version && rustc --version
WORKDIR /

# The default GNU linker needs more than a 4 GB VM when several big test binaries link at once (it was
# killed with signal 9); lld uses far less memory and is faster. preflight-linux.sh points rustc at it.
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends lld \
 && rm -rf /var/lib/apt/lists/*
