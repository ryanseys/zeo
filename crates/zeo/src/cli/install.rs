//! `zeo install` and `zeo gem precompile`: building a package and putting
//! it where a later compile will find it.

use std::path::PathBuf;

use super::*;

/// `zeo install [--bundle-gemfile <path>] [--gem-path <dir>]... [names...]`.
///
/// The verb precompiles the lockfile's gems into the store tier; it never
/// runs Bundler (that is `zeo bundle install`) and never touches the
/// network. Bare names narrow it to those gems.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct InstallCmd {
    pub(crate) gemfile: Option<PathBuf>,
    pub(crate) gem_paths: Vec<PathBuf>,
    pub(crate) names: Vec<String>,
}

pub(crate) fn parse_install(argv: &[String]) -> Result<InstallCmd, String> {
    let mut cmd = InstallCmd::default();
    let mut iter = argv.iter();
    while let Some(arg) = iter.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let mut value = |flag: &str| -> Result<String, String> {
            match inline.clone() {
                Some(v) => Ok(v),
                None => iter
                    .next()
                    .cloned()
                    .ok_or(format!("{flag} requires a value")),
            }
        };
        match name {
            "--bundle-gemfile" => cmd.gemfile = Some(PathBuf::from(value("--bundle-gemfile")?)),
            "--gem-path" => cmd.gem_paths.push(PathBuf::from(value("--gem-path")?)),
            _ if name.starts_with('-') => {
                return Err(format!(
                    "invalid option for zeo install: {name} (it takes --bundle-gemfile, \
                     --gem-path, and gem names)"
                ));
            }
            _ => cmd.names.push(arg.clone()),
        }
    }
    Ok(cmd)
}

/// Compile a package, through the machine-wide package
/// cache. `-o pkg.zeopkg` writes the single-file artifact; any other `-o`
/// writes the raw object with the manifest beside it. A cache write that
/// fails (a read-only cache directory) degrades to "compiled, not cached"
/// with a log line, never to a failed build.
/// `zeo install`: precompile the project's locked gems into the store tier.
///
/// Resolution is Bundler's, already written down -- this verb reads the
/// lockfile and the installed store, compiles each gem it can to a
/// `.zeopkg` (through the machine cache), and places the artifact at its
/// store home. It never runs Bundler and never touches the network; a
/// missing store says to run `zeo bundle install` first. A gem the
/// package tier cannot carry is a `skip` or `declined` row, never an
/// error: its require simply keeps compiling from source.
pub(crate) fn run_install(cmd: InstallCmd) -> Result<(), MainError> {
    init_tracing(None);
    crate::home::ensure_resolved()?;
    let env = Env::from_process();
    let gemfile = cmd
        .gemfile
        .or_else(|| env.bundle_gemfile.as_ref().map(PathBuf::from));
    let project = crate::gems::project::locate(
        gemfile,
        cmd.gem_paths,
        std::env::var_os("GEM_PATH").as_deref(),
    )?;
    let rows = crate::gems::project::survey(&project)?;
    for name in &cmd.names {
        if !rows.iter().any(|r| &r.name == name) {
            return Err(format!("`{name}` is not a gem in {}", project.lockfile.display()).into());
        }
    }
    crate::memguard::arm("install");
    let (mut installed, mut cached_only, mut skipped, mut declined) = (0u32, 0u32, 0u32, 0u32);
    for row in &rows {
        if !cmd.names.is_empty() && !cmd.names.contains(&row.name) {
            continue;
        }
        let label = format!("{} {}", row.name, row.version);
        if let Some(reason) = &row.skip {
            println!("     skip {label} ({reason})");
            skipped += 1;
            continue;
        }
        let (entry, home) = (
            row.entry.as_ref().expect("no skip means an entry"),
            row.home.as_ref().expect("an entry has a home"),
        );
        // A platform gem SHIPPED its artifact: on an exact identity match
        // it is promoted into the store as-is, no compile. A mismatch is
        // not an error -- the compile below is the answer either way.
        if let Some(shipped) = row
            .shipped
            .as_ref()
            .filter(|s| crate::gems::project::artifact_matches(s))
        {
            match crate::packages::package::store_install(home, shipped) {
                Ok(true) => {
                    println!("  install {label} (shipped artifact)");
                    installed += 1;
                    continue;
                }
                // Already where a compile links it from; nothing to do.
                Ok(false) => {
                    println!("    cache {label} (shipped artifact; store not writable)");
                    cached_only += 1;
                    continue;
                }
                Err(e) => {
                    return Err(format!("installing {}: {e}", home.display()).into());
                }
            }
        }
        match install_one(&row.feature, entry, home) {
            Ok(true) => {
                println!("  install {label}");
                installed += 1;
            }
            Ok(false) => {
                println!("    cache {label} (store not writable; kept in the machine cache)");
                cached_only += 1;
            }
            Err(reason) => {
                println!("  decline {label} ({reason})");
                declined += 1;
            }
        }
    }
    println!(
        "zeo install: {installed} installed, {cached_only} cached, {skipped} skipped, \
         {declined} declined"
    );
    Ok(())
}

/// Compile `entry` as the package for `feature` into the `.zeopkg` at
/// `out`, through the machine cache. `Err` carries the refusal's first
/// line -- the drop-to-splice spelling a caller reports.
pub(crate) fn build_gem_artifact(
    feature: &str,
    entry: &std::path::Path,
    out: &std::path::Path,
) -> Result<(), String> {
    let root = entry
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .canonicalize()
        .map_err(|e| format!("resolving {}: {e}", entry.display()))?;
    let opts = crate::CompileOptions {
        input_path: Some(entry.to_path_buf()),
        file_name: Some(PathBuf::from(format!("<package {feature}>"))),
        mode: crate::CompileMode::Program,
        package_build: Some(crate::packages::package::PackageBuild {
            entry: entry.to_path_buf(),
            feature: feature.to_string(),
            manifest_out: out.with_extension("zman"),
            root,
        }),
        ..Default::default()
    };
    // The package build's MAIN source is synthetic and empty; the entry
    // rides in as a feature unit (see the host compile above).
    let first_line = |e: MainError| {
        let text = match e {
            MainError::Plain(m) => m,
            MainError::Compile(e) => e.to_string(),
        };
        text.lines().next().unwrap_or_default().to_string()
    };
    build_package("", &opts, out).map_err(first_line)
}

/// Compile one gem to a `.zeopkg` and place it at its store `home`.
/// `Ok(false)` = built, but the store is read-only (the machine cache still
/// holds it). `Err` carries the refusal's first line.
pub(crate) fn install_one(
    feature: &str,
    entry: &std::path::Path,
    home: &std::path::Path,
) -> Result<bool, String> {
    let staging = std::env::temp_dir().join(format!(
        "zeo-install-{}-{}.zeopkg",
        std::process::id(),
        crate::packages::package::fnv64(home.as_os_str().as_encoded_bytes())
    ));
    build_gem_artifact(feature, entry, &staging)?;
    let placed = crate::packages::package::store_install(home, &staging)
        .map_err(|e| format!("installing {}: {e}", home.display()))?;
    let _ = std::fs::remove_file(&staging);
    Ok(placed)
}

/// `zeo gem precompile`: build THIS gem's platform gem with the artifact
/// inside.
///
/// The author's half of the shipping tier: compile the gem to its
/// `.zeopkg`, put it at `zeo/pkg.zeopkg` in the tree, and have the
/// vendored RubyGems build the platform-stamped `.gem` -- so the file an
/// author pushes is built by RubyGems' own machinery, not an imitation.
/// An installer that cannot use the artifact (other platform, other zeo)
/// ignores it and compiles from source; that contract lives in
/// `crate::gems::project::artifact_matches`.
pub(crate) fn run_gem_precompile() -> Result<(), MainError> {
    init_tracing(None);
    crate::home::ensure_resolved()?;
    let dir = std::env::current_dir().map_err(|e| format!("reading the working directory: {e}"))?;
    let gem = crate::gems::project::author_gem(&dir)?;
    crate::memguard::arm("gem precompile");
    let artifact = dir.join("zeo").join("pkg.zeopkg");
    std::fs::create_dir_all(dir.join("zeo"))
        .map_err(|e| format!("creating {}: {e}", dir.join("zeo").display()))?;
    build_gem_artifact(&gem.feature, &gem.entry, &artifact)?;
    let platform = crate::gems::project::gem_platform();
    let me = std::env::current_exe().map_err(|e| format!("finding the zeo binary: {e}"))?;
    let status = std::process::Command::new(me)
        .arg("-e")
        .arg(crate::gems::bundle::PRECOMPILE_GEM_BUILD)
        .arg("--")
        .arg(&gem.gemspec)
        .arg(&platform)
        .current_dir(&dir)
        .status()
        .map_err(|e| format!("running the gem build: {e}"))?;
    // The artifact travels INSIDE the .gem; the loose copy goes.
    let _ = std::fs::remove_file(&artifact);
    let _ = std::fs::remove_dir(dir.join("zeo"));
    if !status.success() {
        return Err("the gem build failed (see above)".to_string().into());
    }
    println!(
        "zeo gem precompile: {}-{}-{platform}.gem carries the precompiled artifact",
        gem.name, gem.version
    );
    Ok(())
}

pub(crate) fn build_package(
    source: &str,
    opts: &crate::CompileOptions,
    out: &std::path::Path,
) -> Result<(), MainError> {
    let bundled = out.extension().is_some_and(|e| e == "zeopkg");
    let pb = opts.package_build.as_ref().expect("a package build");
    let manifest_out = pb.manifest_out.clone();
    let key = crate::packages::progcache::pkg_key(source, opts);
    if crate::packages::progcache::enabled()
        && let Some(hit) = crate::packages::progcache::pkg_lookup(&key)
    {
        let (manifest_json, object) = crate::packages::package::read_zeopkg(&hit)?;
        return place_package(out, bundled, &manifest_json, &object, &manifest_out);
    }
    let compiled = crate::compile_to_object_with(source, opts, false)?;
    let manifest_json = std::fs::read_to_string(&manifest_out)
        .map_err(|e| format!("reading {}: {e}", manifest_out.display()))?;
    place_package(
        out,
        bundled,
        &manifest_json,
        &compiled.object,
        &manifest_out,
    )?;
    if crate::packages::progcache::enabled() {
        let cached: std::io::Result<()> = (|| {
            let slot = crate::packages::progcache::pkg_reserve(&key)?;
            crate::packages::package::write_zeopkg(&slot, &manifest_json, &compiled.object)?;
            crate::packages::progcache::pkg_commit(&key, &compiled.inputs)
        })();
        if let Err(e) = cached {
            tracing::warn!("could not record the package cache entry: {e}");
        }
    }
    Ok(())
}

/// Land a package's two halves at `-o`.
pub(crate) fn place_package(
    out: &std::path::Path,
    bundled: bool,
    manifest_json: &str,
    object: &[u8],
    manifest_out: &std::path::Path,
) -> Result<(), MainError> {
    if bundled {
        crate::packages::package::write_zeopkg(out, manifest_json, object)
            .map_err(|e| format!("writing {}: {e}", out.display()))?;
        // The bundle carries the manifest inside; a compile may have left
        // the loose copy beside the output.
        let _ = std::fs::remove_file(manifest_out);
    } else {
        std::fs::write(out, object).map_err(|e| format!("writing {}: {e}", out.display()))?;
        std::fs::write(manifest_out, manifest_json)
            .map_err(|e| format!("writing {}: {e}", manifest_out.display()))?;
    }
    Ok(())
}
