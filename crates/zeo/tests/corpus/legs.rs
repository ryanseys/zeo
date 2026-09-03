//! The legs a program can take: the same file, a different question.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::case::Case;
use crate::suites::Suite;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leg {
    /// The in-process JIT: compile and run in memory, nothing on disk.
    Jit,
    /// The same CLIF through an object file and a real link: what ships.
    Aot,
    /// The JIT under the ownership ledger, the cycle collector and the exit
    /// census.
    Memcheck,
    /// The JIT, then the same program with typed emission off, with packaged
    /// ids forced, and down the packaged-cache road; every answer must equal
    /// the first.
    Differential,
}

impl Leg {
    pub fn is_aot(self) -> bool {
        self == Leg::Aot
    }

    fn backend(self) -> &'static str {
        match self {
            Leg::Aot => "aot",
            Leg::Jit | Leg::Memcheck | Leg::Differential => "jit",
        }
    }
}

/// Which road the spawned CLI takes: an explicit `--backend`, or the default
/// cache road with auto-packaging on against a pinned program/package cache
/// pair (the spliced-vs-packaged question).
pub enum Road<'a> {
    Backend(&'a str),
    PackagedCache {
        programs: &'a Path,
        packages: &'a Path,
    },
}

/// The `zeo` command for `case`, on `leg`, with an optional extra
/// `ZEO_DEBUG` flag (the differential children).
pub fn zeo_command(
    leg: Leg,
    road: Road<'_>,
    case: &Case,
    rb: &Path,
    suite: &Suite,
    run_cwd: &Path,
    extra_debug: Option<&str>,
) -> Result<Command, String> {
    let mut cmd = Command::new(crate::common::zeo_cli()?);
    match road {
        Road::Backend(backend) => {
            cmd.arg("--backend").arg(backend);
        }
        Road::PackagedCache { programs, packages } => {
            // ZEO_CACHE=1 beats an ambient off switch: this road exists to
            // run the cache.
            cmd.env("ZEO_CACHE", "1")
                .env("ZEO_PROGRAM_CACHE", programs)
                .env("ZEO_PACKAGE_CACHE", packages);
        }
    }
    if let Some(flag) = extra_debug {
        let ambient = std::env::var("ZEO_DEBUG").unwrap_or_default();
        let joined = if ambient.is_empty() {
            flag.to_string()
        } else {
            format!("{ambient},{flag}")
        };
        cmd.env("ZEO_DEBUG", joined);
    }
    if suite.whole_graph {
        for root in oracle_store_rspec_libs(&crate::common::workspace_root()) {
            cmd.arg("-I").arg(root);
        }
    }
    cmd.args(&case.directives.zeo);
    // Exactly what the oracle gets: the file, then the program's own args.
    // Option parsing stops at the file name on both sides, so a `--seed 42`
    // reaches the program without a separator.
    cmd.arg(rb).args(&case.directives.args);
    for (k, v) in &case.directives.env {
        cmd.env(k, v);
    }
    for (k, v) in &case.directives.zeo_env {
        cmd.env(k, v);
    }
    if leg == Leg::Memcheck {
        cmd.env("ZEO_RT_LEAKCHECK", "1")
            .env("ZEO_GC", "1")
            .env("ZEO_RT_GCCHECK", "1");
    }
    crate::common::child_env(&mut cmd)?;
    cmd.current_dir(run_cwd);
    Ok(cmd)
}

/// The default backend for a leg.
pub fn road_for(leg: Leg) -> Road<'static> {
    Road::Backend(leg.backend())
}

/// The oracle store's rspec trees, for the zeo side of the milestone that
/// runs a real suite. The oracle reaches them through bundler; zeo needs `-I`
/// on each `lib/`. Scoped to rspec rather than the whole store, which would
/// put upstream copies of gems zeo ships ahead of its own.
pub fn oracle_store_rspec_libs(repo: &Path) -> Vec<PathBuf> {
    let gems = repo.join("vendor").join("bundle").join("ruby");
    let mut libs: Vec<PathBuf> = std::fs::read_dir(&gems)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|abi| {
            std::fs::read_dir(abi.path().join("gems"))
                .into_iter()
                .flatten()
        })
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("rspec") || n.starts_with("diff-lcs")
            })
        })
        .map(|p| p.join("lib"))
        .filter(|p| p.is_dir())
        .collect();
    libs.sort();
    libs
}

/// The persistent cache pair the packaged road pins, under the scratch root
/// (which is already keyed on this build of the compiler). Within one build
/// the caches persist, so a gem packages once and every later program links
/// it.
pub fn packaged_cache() -> Result<(PathBuf, PathBuf), String> {
    let root = crate::common::scratch_root()?.join("pkgcache");
    let pair = (root.join("programs"), root.join("packages"));
    std::fs::create_dir_all(&pair.0).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&pair.1).map_err(|e| e.to_string())?;
    Ok(pair)
}
