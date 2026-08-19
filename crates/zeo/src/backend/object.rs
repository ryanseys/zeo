//! Object-file plumbing for the AOT backend: put the emitted bytes on disk
//! and hand them to the link driver.

use std::path::Path;

/// Write `object` to a scratch `.o` beside the output, link it against
/// `libzeo.a`, and clean up. The scratch name carries the pid so parallel
/// `zeo` processes never collide.
pub fn object_to_binary(object: &[u8], output: &Path) -> Result<(), String> {
    let obj_path = std::env::temp_dir().join(format!("zeo-p0-{}.o", std::process::id()));
    std::fs::write(&obj_path, object)
        .map_err(|e| format!("writing {}: {e}", obj_path.display()))?;
    let linked = super::link::link_binary(&obj_path, output);
    let _ = std::fs::remove_file(&obj_path);
    linked
}
