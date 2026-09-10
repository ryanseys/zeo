//! Writing: what reaches the descriptor, and how `print`/`puts`/`p`
//! render their arguments before it does.

use super::*;

/// The `RIo` backend arms every writer shares -- BYTES in, so a BINARY
/// string's raw bytes reach the fd untouched (the display pipeline's
/// `to_utf8_lossy` promotes `0xB4` to `0xC2 0xB4`, which corrupted every
/// binary-image benchmark's output; see `write_value`).
pub(super) fn write_rio(io: &RIo, bytes: &[u8]) -> Result<(), Signal> {
    // BEFORE the descriptor is touched: a read-only handle written to is an
    // `IOError` in ruby, not the kernel's `EBADF`.
    if matches!(access_mode(io), Some((_, false))) {
        return Err(io_error!("not opened for writing"));
    }
    // Gvl-released like `with_file`: a write to a full pipe blocks until
    // the reader drains it, and an armed holder must not stall siblings
    // behind that.
    crate::gvl::without_gvl(|| match &mut *io.backend.lock() {
        IoBackend::Std(StdStream::Stdout) => {
            let mut out = std::io::stdout();
            if out.write_all(bytes).is_err() {
                // A closed pipe downstream (`head`, etc.) -- CRuby
                // dies with EPIPE; a quiet exit is the pragmatic
                // equivalent here.
                std::process::exit(0);
            }
            Ok(())
        }
        IoBackend::Std(StdStream::Stderr) => {
            let _ = std::io::stderr().write_all(bytes);
            Ok(())
        }
        IoBackend::Std(StdStream::Stdin) => Err(io_error!("not opened for writing")),
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::Uninit => Err(io_error!("uninitialized stream")),
        IoBackend::File(Some(f)) | IoBackend::Pipe(Some(f)) => blocking_write_all(f, bytes)
            .map_err(|e| {
                crate::builtins::file::raise_errno(
                    &e,
                    "write",
                    io.path.lock().as_deref().unwrap_or_default(),
                )
            }),
    })
}

/// Writes `s` to `target`: directly for one of our `RIo`s, via a dynamic
/// `write` send for anything else (`$stdout = <duck>` redirection --
/// CRuby's own contract is "any object responding to `write`").
pub fn write_str(target: &RubyValue, s: &str) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target
        && let Some(io) = o.as_any().downcast_ref::<RIo>()
    {
        return write_rio(io, s.as_bytes());
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::wk::write(),
        &[RubyValue::Str(crate::collections::string_new(
            s.to_string(),
        ))],
        None,
    )
    .map(|_| ())
}

/// `write_str` for an already-assembled BYTE buffer (the `print`/`puts`
/// accumulators, `putc`'s single byte). A duck target receives it as a
/// BINARY string -- the honest tag for bytes with no other provenance.
pub fn write_bytes(target: &RubyValue, bytes: &[u8]) -> Result<(), Signal> {
    if let RubyValue::Object(o) = target
        && let Some(io) = o.as_any().downcast_ref::<RIo>()
    {
        return write_rio(io, bytes);
    }
    crate::dispatch::send_value(
        target,
        crate::symbol::wk::write(),
        &[RubyValue::Str(crate::collections::string_from_bytes(
            bytes.to_vec(),
            crate::encoding::ASCII_8BIT,
        ))],
        None,
    )
    .map(|_| ())
}

/// Writes one Ruby VALUE the way `IO#write`/`#<<` must: a String
/// contributes its RAW bytes in its own encoding (a duck target gets the
/// very same String object, exactly CRuby); anything else goes through the
/// display rendering. Answers the BYTE count written (`IO#write`'s return
/// contract). This is the seam that keeps `0xB4` one byte instead of the
/// display pipeline's Latin-1 -> UTF-8 promotion.
pub fn write_value(target: &RubyValue, v: &RubyValue) -> Result<i64, Signal> {
    if let RubyValue::Str(s) = v {
        let bytes = {
            let b = s.lock();
            b.bytes().to_vec()
        };
        if let RubyValue::Object(o) = target
            && let Some(io) = o.as_any().downcast_ref::<RIo>()
        {
            write_rio(io, &bytes)?;
            return Ok(bytes.len() as i64);
        }
        crate::dispatch::send_value(
            target,
            crate::symbol::wk::write(),
            std::slice::from_ref(v),
            None,
        )?;
        return Ok(bytes.len() as i64);
    }
    let s = v.try_display_string()?;
    write_str(target, &s)?;
    Ok(s.len() as i64)
}

/// Appends `v`'s printed form to a BYTE buffer: a String's raw bytes in
/// its own encoding, every other value's display rendering (UTF-8). The
/// `print`/`puts` family accumulates through this so binary strings
/// survive to the fd byte-for-byte.
pub fn display_bytes(v: &RubyValue, buf: &mut Vec<u8>) -> Result<(), Signal> {
    match v {
        RubyValue::Str(s) => buf.extend_from_slice(s.lock().bytes()),
        // Fallible: a user `to_s` that raises propagates out of the
        // `print`/`puts` family as a catchable exception (CRuby's rule).
        other => buf.extend_from_slice(other.try_display_string()?.as_bytes()),
    }
    Ok(())
}

/// `puts`'s rendering into a BYTE buffer (a String arg contributes its raw
/// bytes -- see `display_bytes`): every arg on its own line, arrays
/// flattened recursively, `[...]` for a self-referential array, a bare
/// newline for no args / an empty array -- CRuby's exact shapes.
pub fn render_puts(args: &[RubyValue], buf: &mut Vec<u8>) -> Result<(), Signal> {
    fn put_one(v: &RubyValue, seen: &mut Vec<usize>, buf: &mut Vec<u8>) -> Result<(), Signal> {
        match v {
            RubyValue::Array(a) => {
                let id = Arc::as_ptr(a) as usize;
                if seen.contains(&id) {
                    buf.extend_from_slice(b"[...]\n");
                    return Ok(());
                }
                seen.push(id);
                let items = a.lock().clone();
                // An empty array contributes nothing (CRuby's `io_puts_ary`
                // loops zero times); only zero-arg `puts` writes a bare
                // newline -- oracle-verified `puts []` prints nothing.
                for e in &items {
                    put_one(e, seen, buf)?;
                }
                seen.pop();
            }
            other => {
                let start = buf.len();
                display_bytes(other, buf)?;
                if buf.len() == start || buf.last() != Some(&b'\n') {
                    buf.push(b'\n');
                }
            }
        }
        Ok(())
    }
    if args.is_empty() {
        buf.push(b'\n');
    }
    // One reusable cycle-guard: it is empty between top-level args by
    // construction (push/pop pairs), so sharing it never links siblings.
    let mut seen = Vec::new();
    for a in args {
        put_one(a, &mut seen, buf)?;
    }
    Ok(())
}

/// [`blocking_read`]'s write twin: `write_all` over a descriptor that may be
/// non-blocking, waiting for room in a full pipe instead of failing.
pub(super) fn blocking_write_all(f: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::ErrorKind::{Interrupted, WouldBlock, WriteZero};
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::io::Write::write(f, rest) {
            Ok(0) => return Err(WriteZero.into()),
            Ok(n) => rest = &rest[n..],
            Err(e) if e.kind() == Interrupted => continue,
            Err(e) if e.kind() == WouldBlock => wait_ready(f, libc::POLLOUT)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// The bytes `putc` writes for its argument: a String's FIRST CHARACTER in
/// the string's own encoding (one raw byte for the byte encodings -- never
/// the display pipeline's Latin-1 -> UTF-8 promotion), an Integer's low
/// byte. NUM2CHR for everything else (`putc 2.5` truncates; a `to_str`
/// duck does NOT apply here -- oracle-verified).
pub(crate) fn putc_bytes(arg: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(match arg {
        RubyValue::Str(s) => {
            let b = s.lock();
            match b.encoding().kind() {
                crate::encoding::EncKind::Latin1
                | crate::encoding::EncKind::Binary
                | crate::encoding::EncKind::Registered
                | crate::encoding::EncKind::SingleByte => {
                    b.bytes().first().map(|&x| vec![x]).unwrap_or_default()
                }
                // First CHARACTER in the string's own encoding -- a
                // multibyte sequence stays its raw bytes.
                crate::encoding::EncKind::MultiByte(_)
                | crate::encoding::EncKind::Utf16 { .. }
                | crate::encoding::EncKind::Utf32 { .. } => {
                    b.char_at(0).map(|c| c.bytes().to_vec()).unwrap_or_default()
                }
                crate::encoding::EncKind::Utf8 | crate::encoding::EncKind::Ascii => b
                    .to_utf8_lossy()
                    .chars()
                    .next()
                    .map(|c| c.to_string().into_bytes())
                    .unwrap_or_default(),
            }
        }
        other => vec![(convert::to_index(other)? & 0xff) as u8],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::str_value;

    /// A BINARY-tagged string holding raw bytes -- what `Integer#chr`
    /// (128..=255), `String#b`, and binary IO reads produce.
    fn bin(bytes: &[u8]) -> RubyValue {
        RubyValue::Str(crate::collections::string_from_bytes(
            bytes.to_vec(),
            crate::encoding::ASCII_8BIT,
        ))
    }

    // --- display_bytes: the print-family accumulator ------------------

    #[test]
    fn display_bytes_keeps_a_binary_strings_raw_bytes() {
        // THE regression this seam exists for: `0xB4` must stay one byte,
        // not the Latin-1 -> UTF-8 promotion `0xC2 0xB4` that corrupted
        // bm_ao_render/bm_so_mandelbrot's image output.
        let mut buf = Vec::new();
        display_bytes(&bin(&[0xb4]), &mut buf).unwrap();
        assert_eq!(buf, [0xb4]);
    }

    #[test]
    fn display_bytes_renders_utf8_strings_and_non_strings_as_display_text() {
        let mut buf = Vec::new();
        display_bytes(&str_value("héllo"), &mut buf).unwrap();
        display_bytes(&RubyValue::Int(42), &mut buf).unwrap();
        display_bytes(&RubyValue::Nil, &mut buf).unwrap(); // `print nil` -> ""
        assert_eq!(buf, "héllo42".as_bytes());
    }

    #[test]
    fn display_bytes_keeps_every_byte_of_a_longer_binary_string() {
        let mut buf = Vec::new();
        display_bytes(&bin(&[0x00, 0x7f, 0x80, 0xff]), &mut buf).unwrap();
        assert_eq!(buf, [0x00, 0x7f, 0x80, 0xff]);
    }

    // --- render_puts: CRuby's exact line shapes, now byte-faithful ----

    #[test]
    fn render_puts_writes_a_bare_newline_for_no_args_but_nothing_for_empty_arrays() {
        let mut buf = Vec::new();
        render_puts(&[], &mut buf).unwrap();
        assert_eq!(buf, b"\n");
        buf.clear();
        render_puts(&[RubyValue::Array(crate::array_new(Vec::new()))], &mut buf).unwrap();
        assert_eq!(buf, b"");
    }

    #[test]
    fn render_puts_adds_one_newline_and_never_doubles_a_trailing_one() {
        let mut buf = Vec::new();
        render_puts(&[str_value("a"), str_value("b\n")], &mut buf).unwrap();
        assert_eq!(buf, b"a\nb\n");
    }

    #[test]
    fn render_puts_flattens_nested_arrays_recursively() {
        let inner = RubyValue::Array(crate::array_new(vec![str_value("b"), str_value("c")]));
        let outer = RubyValue::Array(crate::array_new(vec![str_value("a"), inner]));
        let mut buf = Vec::new();
        render_puts(&[outer], &mut buf).unwrap();
        assert_eq!(buf, b"a\nb\nc\n");
    }

    #[test]
    fn render_puts_preserves_binary_bytes_and_still_terminates_the_line() {
        let mut buf = Vec::new();
        render_puts(&[bin(&[0xb4])], &mut buf).unwrap();
        assert_eq!(buf, [0xb4, b'\n']);
        // A binary string ENDING in 0x0A already has its line ending.
        buf.clear();
        render_puts(&[bin(&[0xb4, b'\n'])], &mut buf).unwrap();
        assert_eq!(buf, [0xb4, b'\n']);
    }

    #[test]
    fn render_puts_marks_a_self_referential_array_instead_of_recursing() {
        let arr = crate::array_new(vec![str_value("a")]);
        arr.lock().push(RubyValue::Array(arr.clone()));
        let mut buf = Vec::new();
        render_puts(&[RubyValue::Array(arr)], &mut buf).unwrap();
        assert_eq!(buf, b"a\n[...]\n");
    }

    // --- putc_bytes: one character, in the argument's own encoding ----

    #[test]
    fn putc_bytes_takes_an_integers_low_byte() {
        assert_eq!(putc_bytes(&RubyValue::Int(0xb4)).unwrap(), [0xb4]);
        assert_eq!(putc_bytes(&RubyValue::Int(0x1234)).unwrap(), [0x34]);
        // NUM2CHR truncates a Float (oracle-verified).
        assert_eq!(putc_bytes(&RubyValue::Float(65.9)).unwrap(), [65]);
    }

    #[test]
    fn putc_bytes_takes_a_strings_first_character_in_its_own_encoding() {
        // UTF-8: the first CHARACTER (multibyte stays whole).
        assert_eq!(putc_bytes(&str_value("ab")).unwrap(), b"a");
        assert_eq!(putc_bytes(&str_value("éx")).unwrap(), "é".as_bytes());
        // BINARY: exactly one raw byte, no UTF-8 promotion.
        assert_eq!(putc_bytes(&bin(&[0xb4, 0x01])).unwrap(), [0xb4]);
        assert_eq!(putc_bytes(&str_value("")).unwrap(), Vec::<u8>::new());
    }

    #[test]
    #[should_panic(expected = "no implicit conversion from nil to integer")]
    fn putc_bytes_rejects_a_non_character_argument() {
        // NUM2CHR raises CRuby's TypeError; with no registry installed the
        // unit context surfaces it through `raise_error`'s panic fallback,
        // message intact.
        let _ = putc_bytes(&RubyValue::Nil);
    }

    // --- write_value / write_bytes: the fd-facing seam ----------------

    /// A File-backed `RIo` over a fresh temp file, plus its path for
    /// reading the bytes back.
    fn temp_file_io(tag: &str) -> (RubyValue, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("zeo_rt_io_test_{tag}_{}", std::process::id()));
        let f = std::fs::File::create(&path).expect("temp file");
        (file_value(f, Some(path.display().to_string())), path)
    }

    #[test]
    fn write_value_sends_a_binary_strings_raw_bytes_and_counts_them() {
        let (io, path) = temp_file_io("binary");
        let n = write_value(&io, &bin(&[0x00, 0xb4, 0xff])).unwrap();
        assert_eq!(n, 3);
        assert_eq!(std::fs::read(&path).unwrap(), [0x00, 0xb4, 0xff]);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_value_counts_a_utf8_strings_bytes_and_renders_non_strings() {
        let (io, path) = temp_file_io("mixed");
        assert_eq!(write_value(&io, &str_value("é")).unwrap(), 2);
        assert_eq!(write_value(&io, &RubyValue::Int(42)).unwrap(), 2);
        assert_eq!(std::fs::read(&path).unwrap(), "é42".as_bytes());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn write_bytes_reaches_the_backend_untouched() {
        let (io, path) = temp_file_io("bytes");
        write_bytes(&io, &[0xc2, 0xb4, 0x00]).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0xc2, 0xb4, 0x00]);
        let _ = std::fs::remove_file(path);
    }
}
