//! Object-file plumbing for the AOT backend: put the emitted bytes on disk
//! and hand them to the link driver.

use std::path::Path;

/// Write `object` to a scratch `.o` beside the output, link it against
/// `libzeo.a`, and clean up. The scratch name carries the pid so parallel
/// `zeo` processes never collide.
///
/// `debuginfo` moves the object and keeps it. Mach-O does not put DWARF
/// in the binary at all: the linker records a DEBUG MAP naming the object
/// file it came from, and lldb (or `dsymutil`) reads the DWARF back out
/// of that file at the recorded path. A scratch `.o` in the temp dir,
/// deleted a moment later, is a path to nothing.
pub fn object_to_binary(
    object: &[u8],
    debuginfo: bool,
    loads_cext: bool,
    extra_objects: &[std::path::PathBuf],
    link_args: &[String],
    output: &Path,
) -> Result<(), super::link::LinkError> {
    let obj_path = if debuginfo {
        let mut p = output.to_path_buf();
        let name = p
            .file_name()
            .map_or_else(|| "a.out".to_string(), |n| n.to_string_lossy().into_owned());
        p.set_file_name(format!("{name}.o"));
        p
    } else {
        std::env::temp_dir().join(format!("{}.o", super::scratch_name("zeo-p0")))
    };
    std::fs::write(&obj_path, object).map_err(|e| super::link::LinkError::Io {
        what: format!("writing {}", obj_path.display()),
        detail: e.to_string(),
    })?;
    let linked = super::link::link_binary(
        &obj_path,
        extra_objects,
        link_args,
        output,
        debuginfo,
        loads_cext,
    );
    if !debuginfo {
        let _ = std::fs::remove_file(&obj_path);
    }
    linked
}
