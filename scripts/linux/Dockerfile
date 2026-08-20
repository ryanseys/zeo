# The Linux verification image: what `scripts/linux/verify.sh` runs zeo in.
#
# zeo is developed on macOS, so Linux is the platform every linkage change
# is unverified on until this runs -- a different linker (`ld` not `ld64`),
# a different whole-archive spelling (`--whole-archive` not `-force_load`),
# a different dead-strip flag (`--gc-sections` not `-dead_strip`), and a
# different native-library table (`backend/link.rs::NATLIBS_LINUX_GNU`).
# GitHub Actions covers this too, when it runs; this image is the loop that
# does not depend on that.
#
# Build it once, keep it: the toolchain is NOT baked in. `rust-toolchain.toml`
# in the mounted repo pins the version, and rustup honours it on first use,
# so a toolchain bump needs no image rebuild.
#
#   podman build --platform linux/arm64 -t zeo-linux -f scripts/linux/Dockerfile .
#
# Pin the platform. After ANY amd64 pull, podman resolves `rust:latest` to
# the cached amd64 image, and x86_64 rustc SIGSEGVs under qemu on an Apple
# Silicon host. x86_64 coverage goes through cross-compilation from the
# arm64 container instead (see verify.sh's `cross` stage), never emulation.
FROM docker.io/library/rust:latest

# libclang-dev: ruby-prism-sys runs bindgen, and the rust image ships no
#   libclang (GitHub runners do, which is why CI never needed this line).
# valgrind: the ownership check the macOS leg cannot run -- ZEO_RT_LEAKCHECK
#   proves the emitter's own ledger balances, valgrind proves the process
#   leaks nothing underneath it.
# gcc-x86-64-linux-gnu: the cross-compile linker for the x86_64 stage.
# file/binutils: reading what came out of a link (`file`, `nm`, `readelf`).
RUN apt-get update -qq \
 && apt-get install -y -qq --no-install-recommends \
      libclang-dev valgrind gcc-x86-64-linux-gnu file binutils \
 && rm -rf /var/lib/apt/lists/*

# nextest is the meter every zeo suite is run through (plain `cargo test`
# is order-dependent for zeo-rt). The prebuilt binary, because building it
# from source costs more than everything else in this image.
RUN curl -LsSf https://get.nexte.st/latest/linux-arm | tar zxf - -C /usr/local/bin

# The repo mounts read-only; everything written goes to CARGO_TARGET_DIR
# (a named volume, so rebuilds stay incremental) or /tmp.
ENV CARGO_TARGET_DIR=/target
WORKDIR /src
