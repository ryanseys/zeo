# Verify on Linux

zeo is developed on macOS, so every linkage change is unverified on Linux
until this runs: a different linker, a different whole-archive spelling, a
different dead-strip flag, and a different native-library table.

## Build the image, once

```console
$ podman build --platform linux/arm64 -t zeo-linux .
```

Pin the platform. After any amd64 pull, podman resolves `rust:latest` to the
cached amd64 image, and x86_64 rustc segfaults under emulation on an Apple
Silicon host — x86_64 coverage goes through the `cross` stage instead. On an
x86_64 host, build with `--platform linux/amd64 --build-arg NEXTEST_ARCH=linux`.

The toolchain is not baked in: `rust-toolchain.toml` in the mounted repo
pins it and rustup honours it on first use, so a toolchain bump needs no
rebuild. There is no ruby in the image, and nothing in it needs one.

## Run a stage

```console
$ cargo xtask linux build      # cargo xtask deps, then cargo build --workspace
$ cargo xtask linux test       # the dev loop
$ cargo xtask linux gate       # -P full
$ cargo xtask linux all        # build test units natlibs valgrind
```

| Stage | What it answers |
|---|---|
| `build` | does it compile at all |
| `test` | the corpus and the api suite |
| `gate` | the full profile, at a release boundary |
| `units` | the zeo, zeo-rt and zeo-capi unit suites |
| `natlibs` | does `link.rs`'s glibc table match what rustc says |
| `valgrind` | does a linked program leak underneath the runtime's own ledger |
| `cross` | does the x86_64 cross-compile still link |
| `dist` | does a linux release tarball build |
| `shell` | an interactive prompt in the image |

Triage a red run with a filter:

```console
$ cargo xtask linux test -E 'test(core::string)'
```

Logs land under `target/linux-logs/`.
