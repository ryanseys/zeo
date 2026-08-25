//! The insta PILOT (overhaul decision 2): ~10 goldens copied into
//! `tests/insta-pilot/`, stored as co-located insta `.snap` files instead
//! of `.expected` sidecars, so the two formats can be compared on real
//! cases before any bulk-migration decision. The originals still run in
//! their own suites -- deleting this pilot is `rm -r tests/insta-pilot`
//! plus one `[[test]]` block.
//!
//! The golden ENGINE is shared (`zeo_tests::golden::pilot_run` /
//! `pilot_reference`): sidecars, skips, the spawned child, normalization,
//! and the `.gccheck` census gate all behave identically; only the
//! store-and-diff tail differs. Oracle authority is preserved:
//! `tools/zeo-dev bless <filter>` is still the only writer -- under
//! `ZEO_BLESS_FROM_TOOL` the pilot records the ORACLE's body (zeo's own
//! for a `.divergence` case) by asserting it under `INSTA_UPDATE=always`.
//! A bare `INSTA_UPDATE`/`cargo insta accept` without the bless handshake
//! panics, closing the write path `.expected` never had.

use std::path::Path;

use zeo_tests::golden;

/// The pilot corpus and snapshot home, one directory.
fn pilot_dir() -> std::path::PathBuf {
    golden::workspace_root().join("tests").join("insta-pilot")
}

fn pilot(rb: &Path) -> datatest_stable::Result<()> {
    let blessing = std::env::var_os("ZEO_BLESS_FROM_TOOL").is_some();
    // The one writer rule: insta's own update modes are a second write path
    // the `.expected` system never had, so they are refused outside the
    // bless handshake and forced off inside an ordinary run.
    if std::env::var_os("INSTA_UPDATE").is_some() && !blessing {
        panic!(
            "INSTA_UPDATE is set outside the bless handshake -- \
             use `tools/zeo-dev bless <filter>` to re-record pilot snapshots"
        );
    }
    let stem = rb
        .file_stem()
        .expect("a .rb case has a stem")
        .to_string_lossy()
        .into_owned();
    let run_cwd = golden::tests_run_cwd();

    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(pilot_dir());
    settings.set_prepend_module_to_snapshot(false);
    settings.set_omit_expression(true);

    if blessing {
        let reference = golden::pilot_reference(rb, &run_cwd)?;
        // SAFETY: nextest runs each case in its own process; nothing else
        // reads INSTA_UPDATE concurrently.
        unsafe { std::env::set_var("INSTA_UPDATE", "always") };
        settings.bind(|| insta::assert_snapshot!(stem.as_str(), reference));
        return Ok(());
    }
    unsafe { std::env::set_var("INSTA_UPDATE", "no") };

    let Some(actual) = golden::pilot_run(rb, &run_cwd)? else {
        return Ok(()); // a platform/leg sidecar skipped it
    };

    // A case with NO committed snapshot runs the live oracle instead --
    // the same fallback the `.expected` system gives a fresh golden.
    if !pilot_dir().join(format!("{stem}.snap")).is_file() {
        let reference = golden::pilot_reference(rb, &run_cwd)?;
        if actual == reference {
            return Ok(());
        }
        return Err(format!(
            "{}: output differs from the live oracle.\n--- oracle ---\n{reference}\n--- zeo ---\n{actual}",
            rb.display()
        )
        .into());
    }

    settings.bind(|| insta::assert_snapshot!(stem.as_str(), actual));
    Ok(())
}

datatest_stable::harness! {
    { test = pilot, root = "../../tests/insta-pilot", pattern = r"^[^/]+\.rb$" },
}
