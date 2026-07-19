//! `File` (CRuby file.c) -- the class-level surface: whole-file read/write,
//! the predicates, and the PURE-PATH family (`basename`/`dirname`/`join`/...,
//! which are string manipulation and never touch the disk).
//!
//! `File < IO` (see the ABI table), so a File INSTANCE answers IO's instance
//! methods through the ordinary MRO walk -- nothing is duplicated here.
//!
//! Every path is handled as bytes-as-String. Once the encoding engine lands
//! (plan E1+), paths become a real encoding concern; today they round-trip
//! through `String` and non-UTF-8 paths are out of reach.

use crate::builtins::{arity, block_or_enum, builtin_methods};
use crate::dispatch::raise_error;
use crate::{RubyValue, Signal};

/// Map an `io::Error` to the Ruby `Errno::*` raise CRuby would make, with
/// its message shape: `No such file or directory @ rb_sysopen - /nope/x`.
/// `syscall` is the C function CRuby names in the message (`rb_sysopen` for
/// opens, `dir_initialize` for Dir) -- it is part of the observable message,
/// so callers pass the one their operation corresponds to.
pub fn raise_errno(e: &std::io::Error, syscall: &str, path: &str) -> Signal {
    use std::io::ErrorKind::*;
    let (class, desc) = match e.kind() {
        NotFound => ("Errno::ENOENT", "No such file or directory"),
        PermissionDenied => ("Errno::EACCES", "Permission denied"),
        AlreadyExists => ("Errno::EEXIST", "File exists"),
        // `raw_os_error` catches the kinds `io::ErrorKind` doesn't name
        // (ENOTDIR/ENOTEMPTY have no stable ErrorKind on every platform).
        _ => match e.raw_os_error() {
            Some(libc::ENOTDIR) => ("Errno::ENOTDIR", "Not a directory"),
            Some(libc::EISDIR) => ("Errno::EISDIR", "Is a directory"),
            Some(libc::ENOTEMPTY) => ("Errno::ENOTEMPTY", "Directory not empty"),
            Some(libc::EXDEV) => ("Errno::EXDEV", "Invalid cross-device link"),
            Some(libc::EINVAL) => ("Errno::EINVAL", "Invalid argument"),
            // An errno with no dedicated class: SystemCallError is its own
            // parent and CRuby's own fallback for unmapped codes.
            _ => ("SystemCallError", "Unknown error"),
        },
    };
    raise_error(class, format!("{desc} @ {syscall} - {path}"))
}

/// A path argument. Real Ruby accepts a String or anything with `to_path`;
/// a non-String without it is a TypeError.
pub fn path_arg(v: &RubyValue, method: &str) -> Result<String, Signal> {
    match v {
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => {
            let to_path = crate::Symbol::intern("to_path");
            if crate::dispatch::responds_to(other.class_id(), to_path, false) {
                if let RubyValue::Str(s) =
                    crate::dispatch::send_value(other, to_path, &[], None)?
                {
                    return Ok(s.lock().to_utf8_lossy().into_owned());
                }
            }
            Err(raise_error(
                "TypeError",
                format!(
                    "no implicit conversion of {} into String (in `{method}')",
                    crate::builtins::class_name_of(other)
                ),
            ))
        }
    }
}

/// Flatten `File.join`'s arguments: a String (or `to_path`-able) becomes one
/// path component; an Array is recursively flattened.
fn collect_join_parts(v: &RubyValue, parts: &mut Vec<String>) -> Result<(), Signal> {
    match v {
        RubyValue::Array(el) => {
            let items: Vec<RubyValue> = el.lock().iter().cloned().collect();
            for e in &items {
                collect_join_parts(e, parts)?;
            }
        }
        other => parts.push(path_arg(other, "join")?),
    }
    Ok(())
}

fn str_val(s: String) -> RubyValue {
    RubyValue::Str(crate::collections::string_new(s))
}

/// The raw bytes to write for a value -- a String's own bytes (so a BINARY
/// string round-trips), else its `to_s`.
fn write_bytes(v: &RubyValue) -> Vec<u8> {
    match v {
        RubyValue::Str(s) => s.lock().bytes().to_vec(),
        other => other.to_display_string().into_bytes(),
    }
}

/// The `(external, internal)` encodings a `File.read` should apply, from its
/// trailing options Hash: `mode: "...b..."` forces binary; `encoding:` sets
/// the external (or `"ext:int"` both); `external_encoding:`/
/// `internal_encoding:` override each. Defaults to
/// `(Encoding.default_external, nil)`.
#[allow(clippy::type_complexity)]
fn read_encodings(
    trailing: Option<&RubyValue>,
) -> Result<(crate::encoding::EncodingId, Option<crate::encoding::EncodingId>), Signal> {
    let mut ext = crate::encoding::default_external();
    let mut int = None;
    let Some(RubyValue::Hash(h)) = trailing else {
        return Ok((ext, int));
    };
    let get = |name: &str| {
        crate::collections::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(name)))
    };
    let resolve = |name: &str| {
        crate::encoding::find(name)
            .ok_or_else(|| raise_error("ArgumentError", format!("unknown encoding name - {name}")))
    };
    if let RubyValue::Str(m) = get("mode") {
        if m.lock().to_utf8_lossy().contains('b') {
            ext = crate::encoding::ASCII_8BIT;
        }
    }
    match get("encoding") {
        RubyValue::Str(s) => {
            let name = s.lock().to_utf8_lossy().into_owned();
            match name.split_once(':') {
                Some((e, i)) => {
                    ext = resolve(e)?;
                    int = Some(resolve(i)?);
                }
                None => ext = resolve(&name)?,
            }
        }
        v if !v.is_nil() => ext = crate::builtins::encoding::arg_encoding(&v)?,
        _ => {}
    }
    let external = get("external_encoding");
    if !external.is_nil() {
        ext = crate::builtins::encoding::arg_encoding(&external)?;
    }
    let internal = get("internal_encoding");
    if !internal.is_nil() {
        int = Some(crate::builtins::encoding::arg_encoding(&internal)?);
    }
    Ok((ext, int))
}

/// Builds a read string: bytes tagged with `external`, transcoded to
/// `internal` when one is set (and different).
fn build_read_string(
    bytes: Vec<u8>,
    external: crate::encoding::EncodingId,
    internal: Option<crate::encoding::EncodingId>,
) -> Result<crate::RStr, Signal> {
    match internal {
        Some(int) if int != external => {
            let opts = crate::encoding::TranscodeOptions::default();
            let out = crate::encoding::transcode(&bytes, external, int, &opts, None)
                .map_err(|e| e.into_signal())?;
            Ok(crate::string_from_bytes(out, int))
        }
        _ => Ok(crate::string_from_bytes(bytes, external)),
    }
}

/// Whether a trailing keyword Hash carries `name: <truthy>`. Keywords reach
/// a builtin as one trailing `RubyValue::Hash` (the G2 convention), so this
/// is the shared reader for the option keywords the File rows accept.
fn kwarg_truthy(trailing: Option<&RubyValue>, name: &str) -> bool {
    let Some(RubyValue::Hash(h)) = trailing else {
        return false;
    };
    let key = RubyValue::Symbol(crate::Symbol::intern(name));
    // `hash_get` answers nil for a miss, which is falsy -- exactly the
    // "absent means off" rule these option keywords want.
    crate::collections::hash_get(h, &key).truthy()
}

/// `File.basename(path)` / `File.basename(path, suffix)`. Pure string work:
/// trailing slashes are stripped first (`File.basename("/a/b/")` is `"b"`),
/// and a `".*"` suffix means "any extension".
fn basename_of(path: &str, suffix: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        // The path was "/" (or all slashes) -- basename is "/".
        return if path.is_empty() { String::new() } else { "/".to_string() };
    }
    let base = trimmed.rsplit('/').next().unwrap_or(trimmed).to_string();
    match suffix {
        None => base,
        Some(".*") => match base.rfind('.') {
            // A leading dot is not an extension (`.bashrc`).
            Some(i) if i > 0 => base[..i].to_string(),
            _ => base,
        },
        Some(sfx) if base.len() > sfx.len() && base.ends_with(sfx) => {
            base[..base.len() - sfx.len()].to_string()
        }
        Some(_) => base,
    }
}

/// `File.dirname` -- everything before the last `/`, or `"."` when there is
/// no `/` at all.
fn dirname_of(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.starts_with('/') { "/".to_string() } else { ".".to_string() };
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        // A single leading slash IS the dirname: `/x` -> `/`.
        Some(0) => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

/// `File.extname` -- the last `.` onward, empty when there is none or when
/// the only dot LEADS the basename (`.bashrc` is a name, not an extension).
///
/// A TRAILING dot is an extension of its own: `File.extname("foo.")` is
/// `"."` (oracle-verified -- and worth stating, because it reads like an
/// off-by-one; `".."` and `"."` are still empty, since their dots lead).
fn extname_of(path: &str) -> String {
    let base = basename_of(path, None);
    // A name of nothing but dots has no extension (`.`, `..`).
    if base.chars().all(|c| c == '.') {
        return String::new();
    }
    match base.rfind('.') {
        Some(i) if i > 0 => base[i..].to_string(),
        _ => String::new(),
    }
}

/// `File.expand_path` -- absolutize against `base` (default: cwd), resolving
/// `~`, `.` and `..` LEXICALLY (no symlink resolution, which is
/// `realpath`'s job, matching CRuby).
fn expand_path_of(path: &str, base: Option<&str>) -> Result<String, Signal> {
    let start = if path.starts_with('/') {
        String::new()
    } else if let Some(rest) = path.strip_prefix('~') {
        // `~` / `~/x` -- HOME. (`~user` is not supported; it needs the
        // passwd database, which is the `etc` extension's job.)
        if rest.is_empty() || rest.starts_with('/') {
            std::env::var("HOME").unwrap_or_default()
        } else {
            return Err(raise_error(
                "ArgumentError",
                format!("can't find user {}", rest.split('/').next().unwrap_or("")),
            ));
        }
    } else {
        match base {
            Some(b) => expand_path_of(b, None)?,
            None => std::env::current_dir()
                .map_err(|e| raise_errno(&e, "getcwd", "."))?
                .to_string_lossy()
                .into_owned(),
        }
    };
    let joined = if path.starts_with('/') {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix('~') {
        format!("{start}{rest}")
    } else {
        format!("{start}/{path}")
    };
    // Resolve `.`/`..` lexically.
    let mut out: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    Ok(format!("/{}", out.join("/")))
}

/// The `File::Stat`-free predicates: `metadata` answers `None` for any error
/// (a missing path, an unreadable parent), which is exactly what every
/// `File.<predicate>?` wants -- they answer false rather than raising.
fn meta(path: &str) -> Option<std::fs::Metadata> {
    std::fs::metadata(path).ok()
}

/// `File.ftype`'s answer: the file type of `path` WITHOUT following a final
/// symlink (`lstat`), as CRuby's fixed strings.
fn ftype_string(path: &str) -> Result<&'static str, Signal> {
    use std::os::unix::fs::FileTypeExt;
    let md = std::fs::symlink_metadata(path).map_err(|e| raise_errno(&e, "lstat", path))?;
    let ft = md.file_type();
    Ok(if ft.is_symlink() {
        "link"
    } else if ft.is_dir() {
        "directory"
    } else if ft.is_file() {
        "file"
    } else if ft.is_fifo() {
        "fifo"
    } else if ft.is_socket() {
        "socket"
    } else if ft.is_char_device() {
        "characterSpecial"
    } else if ft.is_block_device() {
        "blockSpecial"
    } else {
        "unknown"
    })
}

/// A `File.open` mode string (`"r"`, `"w"`, `"a"`, `"r+"`, ... with an
/// optional `b`/`t` suffix, which only matter once encodings exist).
fn open_options(mode: &str) -> Result<std::fs::OpenOptions, Signal> {
    let base: String = mode.chars().filter(|c| *c != 'b' && *c != 't').collect();
    let mut o = std::fs::OpenOptions::new();
    match base.as_str() {
        "r" => o.read(true),
        "r+" => o.read(true).write(true),
        "w" => o.write(true).create(true).truncate(true),
        "w+" => o.read(true).write(true).create(true).truncate(true),
        "a" => o.append(true).create(true),
        "a+" => o.read(true).append(true).create(true),
        // `x` requires the file NOT to exist.
        "wx" | "w+x" => o.write(true).create_new(true),
        _ => {
            return Err(raise_error(
                "ArgumentError",
                format!("invalid access mode {mode}"),
            ))
        }
    };
    Ok(o)
}

builtin_methods! {
    pub(crate) fn lookup_class;

    // `File.open(path, mode = "r")` -- with a block, yields the file and
    // CLOSES it afterwards no matter how the block leaves (return, raise,
    // break), answering the block's value; without one, answers the open
    // file for the caller to close.
    "open" | "new" => fn file_open(_recv, args, block) {
        arity!(args, 1..=3);
        let path = path_arg(&args[0], "open")?;
        let mode = match args.get(1) {
            None => "r".to_string(),
            // A trailing options Hash is accepted and ignored (see readlines).
            Some(RubyValue::Hash(_)) => "r".to_string(),
            Some(v) => path_arg(v, "open")?,
        };
        let f = open_options(&mode)?
            .open(&path)
            .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        let io = crate::builtins::io::file_value(f, path);
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(io);
        };
        // The close must happen on EVERY exit path, which is what makes this
        // the idiom it is -- hence running the block, stashing its outcome,
        // closing, and only then propagating.
        let out = p.call(&[io.clone()]);
        let _ = crate::dispatch::send_value(&io, crate::Symbol::intern("close"), &[], None);
        out
    }
    // The text-read path: bytes are tagged with the EXTERNAL encoding
    // (default `Encoding.default_external`, UTF-8) WITHOUT validation --
    // CRuby's own rule. `encoding:`/`external_encoding:`/`internal_encoding:`
    // options override it; an internal encoding transcodes the bytes.
    "read" => fn file_read(_recv, args, _block) {
        arity!(args, 1..=4);
        let path = path_arg(&args[0], "read")?;
        let bytes = std::fs::read(&path).map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        let (ext, int) = read_encodings(args.last())?;
        Ok(RubyValue::Str(build_read_string(bytes, ext, int)?))
    }
    // `binread` always answers ASCII-8BIT bytes, no transcoding.
    "binread" => fn file_binread(_recv, args, _block) {
        arity!(args, 1..=3);
        let path = path_arg(&args[0], "binread")?;
        let bytes = std::fs::read(&path).map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        Ok(RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT)))
    }
    "binwrite" => fn file_binwrite(_recv, args, _block) {
        arity!(args, 2..=3);
        let path = path_arg(&args[0], "binwrite")?;
        let data = write_bytes(&args[1]);
        std::fs::write(&path, &data).map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        Ok(RubyValue::Int(data.len() as i64))
    }
    "readlines" => fn file_readlines(_recv, args, _block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "readlines")?;
        let bytes = std::fs::read(&path).map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // The same splitter `String#each_line`/`#lines` use -- one rule for
        // what a line is.
        let mut lines = crate::builtins::string::split_lines(&text);
        // `chomp: true` strips the terminators (an `encoding:` keyword is
        // accepted and ignored until the encoding engine lands). Keywords
        // arrive as a trailing Hash -- the G2 convention.
        if kwarg_truthy(args.get(1), "chomp") {
            lines = lines
                .into_iter()
                .map(|l| {
                    let s = l.to_display_string();
                    str_val(s.trim_end_matches('\n').trim_end_matches('\r').to_string())
                })
                .collect();
        }
        Ok(RubyValue::Array(crate::collections::array_new(lines)))
    }
    // `File.foreach(path)` -- yield each line; without a block, an Enumerator.
    // `chomp: true` strips terminators, mirroring `readlines`.
    "foreach" => fn file_foreach(recv, args, block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "foreach")?;
        let p = block_or_enum!(recv, "foreach", args, block);
        let bytes = std::fs::read(&path).map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let chomp = kwarg_truthy(args.get(1), "chomp");
        for line in crate::builtins::string::split_lines(&text) {
            let line = if chomp {
                let s = line.to_display_string();
                str_val(s.trim_end_matches('\n').trim_end_matches('\r').to_string())
            } else {
                line
            };
            p.call(&[line])?;
        }
        Ok(RubyValue::Nil)
    }
    // `File.ftype(path)` -- the `lstat` file-type string (doesn't follow a
    // trailing symlink).
    "ftype" => fn file_ftype(_recv, args, _block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "ftype")?;
        Ok(str_val(ftype_string(&path)?.to_string()))
    }
    "write" => fn file_write(_recv, args, _block) {
        arity!(args, 2..=3);
        let path = path_arg(&args[0], "write")?;
        // Bytes are written VERBATIM (a String emits its own bytes, so a
        // BINARY string round-trips unchanged).
        let data = write_bytes(&args[1]);
        std::fs::write(&path, &data)
            .map_err(|e| raise_errno(&e, "rb_sysopen", &path))?;
        Ok(RubyValue::Int(data.len() as i64))
    }
    "exist?" | "exists?" => fn file_exist_p(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(meta(&path_arg(&args[0], "exist?")?).is_some()))
    }
    "file?" => fn file_file_p(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(
            meta(&path_arg(&args[0], "file?")?).is_some_and(|m| m.is_file()),
        ))
    }
    "directory?" => fn file_directory_p(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(
            meta(&path_arg(&args[0], "directory?")?).is_some_and(|m| m.is_dir()),
        ))
    }
    "size" => fn file_size(_recv, args, _block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "size")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_size", &path))?;
        Ok(RubyValue::Int(m.len() as i64))
    }
    "size?" => fn file_size_p(_recv, args, _block) {
        arity!(args, 1);
        // `size?` answers nil for a missing file AND for an empty one --
        // it is the "is there content" predicate, not a size reader.
        Ok(match meta(&path_arg(&args[0], "size?")?) {
            Some(m) if m.len() > 0 => RubyValue::Int(m.len() as i64),
            _ => RubyValue::Nil,
        })
    }
    "zero?" | "empty?" => fn file_zero_p(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(
            meta(&path_arg(&args[0], "zero?")?).is_some_and(|m| m.len() == 0),
        ))
    }
    "readable?" => fn file_readable_p(_recv, args, _block) {
        arity!(args, 1);
        // Existence is a decent proxy only for the common case; ask the OS.
        let p = path_arg(&args[0], "readable?")?;
        Ok(RubyValue::Bool(access(&p, libc::R_OK)))
    }
    "writable?" => fn file_writable_p(_recv, args, _block) {
        arity!(args, 1);
        let p = path_arg(&args[0], "writable?")?;
        Ok(RubyValue::Bool(access(&p, libc::W_OK)))
    }
    "executable?" => fn file_executable_p(_recv, args, _block) {
        arity!(args, 1);
        let p = path_arg(&args[0], "executable?")?;
        Ok(RubyValue::Bool(access(&p, libc::X_OK)))
    }
    "delete" | "unlink" => fn file_delete(_recv, args, _block) {
        // Variadic: deletes every path given, answers how many.
        let mut n = 0;
        for a in args {
            let path = path_arg(a, "delete")?;
            std::fs::remove_file(&path).map_err(|e| raise_errno(&e, "unlink", &path))?;
            n += 1;
        }
        Ok(RubyValue::Int(n))
    }
    "rename" => fn file_rename(_recv, args, _block) {
        arity!(args, 2);
        let from = path_arg(&args[0], "rename")?;
        let to = path_arg(&args[1], "rename")?;
        std::fs::rename(&from, &to).map_err(|e| raise_errno(&e, "rename", &from))?;
        Ok(RubyValue::Int(0))
    }
    // --- The pure-path family: string work, never touches the disk --------
    "basename" => fn file_basename(_recv, args, _block) {
        arity!(args, 1..=2);
        let path = path_arg(&args[0], "basename")?;
        let suffix = match args.get(1) {
            None => None,
            Some(v) => Some(path_arg(v, "basename")?),
        };
        Ok(str_val(basename_of(&path, suffix.as_deref())))
    }
    "dirname" => fn file_dirname(_recv, args, _block) {
        arity!(args, 1..=2);
        Ok(str_val(dirname_of(&path_arg(&args[0], "dirname")?)))
    }
    "extname" => fn file_extname(_recv, args, _block) {
        arity!(args, 1);
        Ok(str_val(extname_of(&path_arg(&args[0], "extname")?)))
    }
    "split" => fn file_split(_recv, args, _block) {
        arity!(args, 1);
        let p = path_arg(&args[0], "split")?;
        Ok(RubyValue::Array(crate::collections::array_new(vec![
            str_val(dirname_of(&p)),
            str_val(basename_of(&p, None)),
        ])))
    }
    "join" => fn file_join(_recv, args, _block) {
        // `File.join("a", ["b", ["c"]])` flattens arbitrarily nested arrays.
        let mut parts = Vec::new();
        for a in args {
            collect_join_parts(a, &mut parts)?;
        }
        // Ruby collapses a separator at the seam rather than doubling it:
        // `File.join("a/", "/b")` is `"a/b"`.
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i == 0 {
                out.push_str(p);
                continue;
            }
            let left_slash = out.ends_with('/');
            let right_slash = p.starts_with('/');
            match (left_slash, right_slash) {
                (true, true) => out.push_str(p.trim_start_matches('/')),
                (false, false) => {
                    out.push('/');
                    out.push_str(p);
                }
                _ => out.push_str(p),
            }
        }
        Ok(str_val(out))
    }
    "expand_path" => fn file_expand_path(_recv, args, _block) {
        arity!(args, 1..=2);
        let p = path_arg(&args[0], "expand_path")?;
        let base = match args.get(1) {
            None => None,
            Some(v) => Some(path_arg(v, "expand_path")?),
        };
        Ok(str_val(expand_path_of(&p, base.as_deref())?))
    }
    "absolute_path?" => fn file_absolute_path_p(_recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(path_arg(&args[0], "absolute_path?")?.starts_with('/')))
    }
    "mtime" => fn file_mtime(_recv, args, _block) {
        arity!(args, 1);
        let path = path_arg(&args[0], "mtime")?;
        let m = std::fs::metadata(&path).map_err(|e| raise_errno(&e, "rb_file_s_mtime", &path))?;
        let t = m
            .modified()
            .map_err(|e| raise_errno(&e, "rb_file_s_mtime", &path))?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| raise_error("SystemCallError", "mtime before the epoch".to_string()))?;
        Ok(crate::builtins::time::time_from_parts(
            t.as_secs() as i64,
            t.subsec_nanos(),
        ))
    }
}

/// `access(2)` -- the real permission question, rather than inferring from
/// metadata (which would ignore ACLs and the effective uid).
fn access(path: &str, mode: libc::c_int) -> bool {
    let Ok(c) = std::ffi::CString::new(path) else {
        // An interior NUL can't name a file; CRuby raises ArgumentError, but
        // for a predicate `false` is the honest answer.
        return false;
    };
    // SAFETY: `c` is a valid NUL-terminated string for the call's duration.
    unsafe { libc::access(c.as_ptr(), mode) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- The pure-path family: no disk, every case oracle-read ------------

    #[test]
    fn basename_strips_directories_and_an_optional_suffix() {
        assert_eq!(basename_of("/a/b/c.rb", None), "c.rb");
        assert_eq!(basename_of("/a/b/c.rb", Some(".rb")), "c");
        assert_eq!(basename_of("/a/b/c.rb", Some(".*")), "c");
        assert_eq!(basename_of("c.rb", None), "c.rb");
        // A trailing slash is stripped first.
        assert_eq!(basename_of("/a/b/", None), "b");
        assert_eq!(basename_of("/", None), "/");
        // A non-matching suffix is left alone.
        assert_eq!(basename_of("/a/c.rb", Some(".py")), "c.rb");
        // A leading dot is not an extension.
        assert_eq!(basename_of("/a/.bashrc", Some(".*")), ".bashrc");
    }

    #[test]
    fn dirname_answers_dot_when_there_is_no_directory() {
        assert_eq!(dirname_of("/a/b/c.rb"), "/a/b");
        assert_eq!(dirname_of("a"), ".");
        assert_eq!(dirname_of("/x"), "/");
        assert_eq!(dirname_of("/"), "/");
        assert_eq!(dirname_of("a/b"), "a");
    }

    /// Every row oracle-read from ruby 4.0.5 -- including the two that read
    /// like off-by-ones: a TRAILING dot IS an extension (`"foo."` -> `"."`),
    /// while a LEADING one is not (`".bashrc"` -> `""`).
    #[test]
    fn extname_matches_the_oracle() {
        for (path, want) in [
            ("/a/b/c.rb", ".rb"),
            ("x", ""),
            (".bashrc", ""),
            ("foo.", "."),
            ("a.b.c", ".c"),
            ("a..b", ".b"),
            ("foo.rb.", "."),
            (".x.y", ".y"),
            ("..", ""),
            (".", ""),
        ] {
            assert_eq!(extname_of(path), want, "File.extname({path:?})");
        }
    }

    fn s(v: &str) -> RubyValue {
        RubyValue::Str(crate::collections::string_new(v.to_string()))
    }
    fn cls() -> RubyValue {
        RubyValue::Class(spinel_abi::FILE_CLASS)
    }

    #[test]
    fn join_collapses_separators_at_the_seam() {
        let j = |parts: &[&str]| {
            let args: Vec<RubyValue> = parts.iter().map(|p| s(p)).collect();
            file_join(&cls(), &args, None).unwrap().to_display_string()
        };
        assert_eq!(j(&["a", "b", "c"]), "a/b/c");
        assert_eq!(j(&["a/", "b"]), "a/b");
        assert_eq!(j(&["a", "/b"]), "a/b");
        assert_eq!(j(&["a/", "/b"]), "a/b");
        assert_eq!(j(&["/a", "b"]), "/a/b");
        assert_eq!(j(&["a"]), "a");
    }

    /// `File.join` flattens a nested Array argument.
    #[test]
    fn join_flattens_an_array_argument() {
        let arr = RubyValue::Array(crate::collections::array_new(vec![s("b"), s("c")]));
        let got = file_join(&cls(), &[s("a"), arr], None).unwrap();
        assert_eq!(got.to_display_string(), "a/b/c");
    }

    #[test]
    fn split_is_dirname_and_basename() {
        let RubyValue::Array(parts) = file_split(&cls(), &[s("/a/b/c.rb")], None).unwrap() else {
            panic!("expected an Array")
        };
        let got: Vec<String> = parts.lock().iter().map(|p| p.to_display_string()).collect();
        assert_eq!(got, vec!["/a/b".to_string(), "c.rb".to_string()]);
    }

    #[test]
    fn expand_path_absolutizes_and_resolves_dots_lexically() {
        assert_eq!(expand_path_of("b", Some("/a")).unwrap(), "/a/b");
        assert_eq!(expand_path_of("/a/b", None).unwrap(), "/a/b");
        assert_eq!(expand_path_of("../c", Some("/a/b")).unwrap(), "/a/c");
        assert_eq!(expand_path_of("./c", Some("/a")).unwrap(), "/a/c");
        assert_eq!(expand_path_of("/a/./b/../c", None).unwrap(), "/a/c");
    }

    /// `~` expands to HOME.
    #[test]
    fn expand_path_resolves_a_leading_tilde() {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return; // no HOME in this environment; nothing to assert
        }
        assert_eq!(expand_path_of("~", None).unwrap(), home);
        assert_eq!(expand_path_of("~/x", None).unwrap(), format!("{home}/x"));
    }

    // --- The disk-touching surface ---------------------------------------

    /// A temp path unique to this test binary + name, so parallel tests
    /// never collide.
    fn tmp(name: &str) -> String {
        format!(
            "{}/spinel_file_test_{}_{}",
            std::env::temp_dir().to_string_lossy(),
            std::process::id(),
            name
        )
    }

    #[test]
    fn write_then_read_round_trips() {
        let p = tmp("roundtrip");
        let n = file_write(&cls(), &[s(&p), s("hello\n")], None).unwrap();
        assert!(matches!(n, RubyValue::Int(6)));
        let got = file_read(&cls(), &[s(&p)], None).unwrap();
        assert_eq!(got.to_display_string(), "hello\n");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn predicates_answer_false_rather_than_raising_for_a_missing_path() {
        let p = tmp("absent");
        for f in [file_exist_p, file_file_p, file_directory_p, file_zero_p] {
            assert!(
                matches!(f(&cls(), &[s(&p)], None).unwrap(), RubyValue::Bool(false)),
                "a predicate raised or answered true for a missing path"
            );
        }
        // `size?` is nil (not 0, not an error) for a missing path.
        assert!(matches!(
            file_size_p(&cls(), &[s(&p)], None).unwrap(),
            RubyValue::Nil
        ));
    }

    #[test]
    fn predicates_distinguish_files_from_directories() {
        let p = tmp("predicates");
        std::fs::write(&p, "x").unwrap();
        assert!(matches!(file_exist_p(&cls(), &[s(&p)], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(file_file_p(&cls(), &[s(&p)], None).unwrap(), RubyValue::Bool(true)));
        assert!(matches!(
            file_directory_p(&cls(), &[s(&p)], None).unwrap(),
            RubyValue::Bool(false)
        ));
        let dir = std::env::temp_dir().to_string_lossy().into_owned();
        assert!(matches!(
            file_directory_p(&cls(), &[s(&dir)], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert!(matches!(file_file_p(&cls(), &[s(&dir)], None).unwrap(), RubyValue::Bool(false)));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn size_reads_the_byte_count_and_zero_p_detects_empty() {
        let p = tmp("size");
        std::fs::write(&p, "12345").unwrap();
        assert!(matches!(file_size(&cls(), &[s(&p)], None).unwrap(), RubyValue::Int(5)));
        assert!(matches!(file_size_p(&cls(), &[s(&p)], None).unwrap(), RubyValue::Int(5)));
        assert!(matches!(file_zero_p(&cls(), &[s(&p)], None).unwrap(), RubyValue::Bool(false)));

        std::fs::write(&p, "").unwrap();
        assert!(matches!(file_zero_p(&cls(), &[s(&p)], None).unwrap(), RubyValue::Bool(true)));
        // An EMPTY file's `size?` is nil, though its `size` is 0.
        assert!(matches!(file_size_p(&cls(), &[s(&p)], None).unwrap(), RubyValue::Nil));
        assert!(matches!(file_size(&cls(), &[s(&p)], None).unwrap(), RubyValue::Int(0)));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn readlines_keeps_the_newlines() {
        let p = tmp("readlines");
        std::fs::write(&p, "a\nb\nc").unwrap();
        let RubyValue::Array(lines) = file_readlines(&cls(), &[s(&p)], None).unwrap() else {
            panic!("expected an Array")
        };
        let got: Vec<String> = lines.lock().iter().map(|l| l.to_display_string()).collect();
        assert_eq!(got, vec!["a\n".to_string(), "b\n".to_string(), "c".to_string()]);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn delete_and_rename() {
        let a = tmp("rename_from");
        let b = tmp("rename_to");
        std::fs::write(&a, "x").unwrap();
        file_rename(&cls(), &[s(&a), s(&b)], None).unwrap();
        assert!(!std::path::Path::new(&a).exists());
        assert!(std::path::Path::new(&b).exists());
        let n = file_delete(&cls(), &[s(&b)], None).unwrap();
        assert!(matches!(n, RubyValue::Int(1)));
        assert!(!std::path::Path::new(&b).exists());
    }

    /// A missing file raises Errno::ENOENT, not a generic error --
    /// registry-less, `raise_error` surfaces as a panic.
    #[test]
    fn reading_a_missing_file_raises_errno_enoent() {
        let r = std::panic::catch_unwind(|| file_read(&cls(), &[s("/nope/definitely/not")], None));
        assert!(r.is_err());
    }

    /// The errno mapping itself, without needing the exception registry:
    /// registry-less, `raise_error` panics with the class + message it would
    /// have raised, so the panic payload IS the assertion. (The `io::Error`
    /// is built inside the closure -- it isn't `RefUnwindSafe`.)
    #[test]
    fn raise_errno_maps_kinds_to_ruby_classes() {
        let msg = std::panic::catch_unwind(|| {
            let e = std::io::Error::from(std::io::ErrorKind::NotFound);
            raise_errno(&e, "rb_sysopen", "/nope")
        })
        .err()
        .map(|p| {
            p.downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "<non-string panic>".to_string())
        })
        .unwrap_or_default();
        assert!(
            msg.contains("Errno::ENOENT")
                && msg.contains("No such file or directory @ rb_sysopen - /nope"),
            "unexpected raise: {msg}"
        );
    }

    #[test]
    fn a_non_string_path_is_a_type_error() {
        let r = std::panic::catch_unwind(|| file_read(&cls(), &[RubyValue::Int(5)], None));
        assert!(r.is_err());
    }

    #[test]
    fn lookup_finds_the_file_names() {
        assert!(lookup_class("read").is_some());
        assert!(lookup_class("basename").is_some());
        assert!(lookup_class("join").is_some());
        assert!(lookup_class("nope").is_none());
    }
}
