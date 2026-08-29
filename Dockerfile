# The Linux verification image: what `cargo xtask linux` runs zeo in.
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
#   podman build --platform linux/arm64 -t zeo-linux .
#
# Pin the platform. After ANY amd64 pull, podman resolves `rust:latest` to
# the cached amd64 image, and x86_64 rustc SIGSEGVs under qemu on an Apple
# Silicon host. x86_64 coverage goes through cross-compilation from the
# arm64 container instead (see the `cross` stage), never emulation.
# THE ORACLE, and it is the same ruby the macOS side uses: 4.0.6 at revision
# 03b6d3f889, which is what `ruby-headers.lock` pins. The image used to take
# Debian's `ruby` package, and that was 3.3.8 -- so a golden with no committed
# `.expected` was arbitrated by one ruby on macOS and a different one here,
# and a version difference would have read as a zeo bug. Deliberately still
# not zeo's own ruby: an oracle that only works when zeo is correct cannot
# arbitrate zeo when it is not.
#
# Both images are Debian trixie, so the shared libraries match, and
# `rust:latest` leaves /usr/local/bin empty for ruby to land in.
FROM docker.io/library/ruby:4.0.6-trixie AS oracle

FROM docker.io/library/rust:latest
COPY --from=oracle /usr/local/bin/ /usr/local/bin/
COPY --from=oracle /usr/local/lib/ruby/ /usr/local/lib/ruby/
COPY --from=oracle /usr/local/lib/libruby.so* /usr/local/lib/
COPY --from=oracle /usr/local/include/ruby-4.0.0/ /usr/local/include/ruby-4.0.0/
COPY --from=oracle /usr/local/etc/gemrc /usr/local/etc/gemrc
RUN ldconfig && ruby -v

# The container resolves the Gemfile into ITS OWN store. `/src` is mounted
# read-write, so a `bundle install` under the repo's own `.bundle/config`
# would write linux-native gems over the host's macOS ones. `BUNDLE_APP_CONFIG`
# moves bundler's app config out of `/src/.bundle`, which is the only setting
# that wins over a committed `.bundle/config`, and `/bundle` is a named volume
# so the install survives between runs.
ENV BUNDLE_APP_CONFIG=/bundleconf
RUN mkdir -p /bundleconf /bundle \
 && printf 'BUNDLE_PATH: "/bundle"\nBUNDLE_FROZEN: "true"\n' > /bundleconf/config

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

# Writes go to CARGO_TARGET_DIR
# (a named volume, so rebuilds stay incremental) or /tmp.
ENV CARGO_TARGET_DIR=/target
WORKDIR /src
