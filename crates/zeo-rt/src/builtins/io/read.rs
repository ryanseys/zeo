//! Reading: the read-ahead buffer every read goes through, the line and
//! character walks over it, and the three argument readers they share.

use super::*;

pub(crate) fn with_file<T>(
    recv: &RubyValue,
    f: impl FnOnce(&mut std::fs::File, &str) -> Result<T, Signal>,
) -> Result<T, Signal> {
    let Some(io) = as_rio(recv) else {
        return Err(io_error!("not a file"));
    };
    let path = io.path.lock().clone().unwrap_or_default();
    crate::gvl::without_gvl(|| match &mut *io.backend.lock() {
        IoBackend::File(Some(file)) | IoBackend::Pipe(Some(file)) => {
            // Give back whatever the line readers read ahead, so this closure
            // sees the descriptor at the position Ruby believes in. A no-op --
            // and syscall-free -- for any program that never buffered.
            unread(io, file);
            f(file, &path)
        }
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::Uninit => Err(io_error!("uninitialized stream")),
        IoBackend::Std(_) => Err(io_error!("not a file")),
    })
}

/// [`with_file`] for the rows that READ THROUGH the buffer rather than around
/// it: same locking and Gvl release, but no `unread` on the way in, and the
/// `RIo` is passed along so the closure can reach the buffer.
pub(super) fn with_buffered_file<T>(
    recv: &RubyValue,
    f: impl FnOnce(&RIo, &mut std::fs::File, &str) -> Result<T, Signal>,
) -> Result<T, Signal> {
    // The READ funnel, so the mode question is asked once here rather than
    // remembered at each of its rows. Without it `getc` on a write-only
    // handle answered "\u0000" and `getbyte` answered 0 -- a made-up byte,
    // not an error.
    check_readable(recv)?;
    let Some(io) = as_rio(recv) else {
        return Err(io_error!("not a file"));
    };
    let path = io.path.lock().clone().unwrap_or_default();
    crate::gvl::without_gvl(|| match &mut *io.backend.lock() {
        IoBackend::File(Some(file)) | IoBackend::Pipe(Some(file)) => f(io, file, &path),
        IoBackend::File(None) | IoBackend::Pipe(None) => Err(io_error!("closed stream")),
        IoBackend::Uninit => Err(io_error!("uninitialized stream")),
        IoBackend::Std(_) => Err(io_error!("not a file")),
    })
}

/// Rewind the descriptor over bytes [`ReadBuf`] read ahead, and drop them.
///
/// Infallible by construction on a SEEKABLE descriptor: the read-ahead is
/// only ever filled after the seek was proved to work, so the seek back
/// cannot be the first one to fail.
///
/// A NON-seekable one keeps its bytes instead of dropping them. There is
/// nowhere to give them back to -- that is what makes it non-seekable --
/// and `eof?` parks exactly one byte there to answer at all. Clearing it
/// here consumed that byte and handed the next reader the one after.
/// Throw away whatever `#ungetbyte` pushed back -- what a seek does to it.
///
/// `unread` hands the read-ahead back to the descriptor; a pushed-back byte
/// has no descriptor to go back to, so a seek simply loses it, as ruby's does.
pub(super) fn drop_unget(recv: &RubyValue) {
    if let Some(io) = as_rio(recv) {
        let mut rb = io.rbuf.lock();
        if rb.unget > 0 {
            *rb = ReadBuf::default();
        }
    }
}

pub(super) fn unread(io: &RIo, f: &mut std::fs::File) {
    let mut buf = io.rbuf.lock();
    // Only the bytes that came FROM the descriptor can be given back to it.
    // A pushed-back byte was never read, so seeking over it would rewind the
    // file one byte too far -- and it has to survive, because the buffer is
    // the only place it exists.
    let ahead = buf.pending() - buf.unget;
    use std::io::Seek;
    if ahead > 0 && f.seek(std::io::SeekFrom::Current(-(ahead as i64))).is_err() {
        return;
    }
    let keep = buf.pos..buf.pos + buf.unget;
    buf.data = buf.data[keep].to_vec();
    buf.pos = 0;
}

/// How much the buffer reads ahead. One page-ish chunk, matching CRuby's own
/// `IO` buffer size.
pub(super) const READ_BUF: usize = 8192;

/// The next byte Ruby should see, drawing from [`ReadBuf`] and refilling it in
/// `READ_BUF` chunks. `None` at end of file.
///
/// The only place the buffer is filled, and it refuses to fill a descriptor
/// that cannot seek -- a pipe, socket or std stream keeps the byte-at-a-time
/// reads, where read-ahead could not be given back and `#readpartial`'s
/// arrival-shaped semantics would change.
pub(super) fn buffered_byte(io: &RIo, f: &mut std::fs::File) -> std::io::Result<Option<u8>> {
    let mut buf = io.rbuf.lock();
    if buf.pos == buf.data.len() {
        let seekable = match buf.seekable {
            Some(s) => s,
            None => {
                use std::io::Seek;
                let s = f.stream_position().is_ok();
                buf.seekable = Some(s);
                s
            }
        };
        if !seekable {
            let mut one = [0u8; 1];
            return Ok((blocking_read(f, &mut one)? == 1).then_some(one[0]));
        }
        buf.data.resize(READ_BUF, 0);
        let got = blocking_read(f, &mut buf.data)?;
        buf.data.truncate(got);
        buf.pos = 0;
        if got == 0 {
            return Ok(None);
        }
    }
    let b = buf.data[buf.pos];
    buf.pos += 1;
    buf.consumed(1);
    Ok(Some(b))
}

/// Whether this IO holds bytes the descriptor no longer has -- an `eof?`
/// peek, or read-ahead. A row that bypasses the buffer (`BasicSocket#recv`)
/// has to refuse rather than skip them.
pub fn has_buffered_bytes(recv: &RubyValue) -> bool {
    as_rio(recv).is_some_and(|io| io.rbuf.lock().pending() > 0)
}

/// [`blocking_read`] with the peek buffer served FIRST.
///
/// `eof?` on a non-seekable descriptor has to consume a byte to answer, and
/// that byte belongs to whoever reads next -- through `read`, `gets`,
/// `readpartial`, `sysread` or `each_codepoint` alike. Serving it from one
/// place is what makes "peek" different from "lose a byte": a per-row
/// unget stack would only have been drained by the one row that knows
/// about it.
pub(super) fn peeked_read(
    io: Option<&RIo>,
    f: &mut std::fs::File,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    if let Some(io) = io {
        let mut rb = io.rbuf.lock();
        let pending = rb.pending();
        if pending > 0 && !buf.is_empty() {
            let n = pending.min(buf.len());
            buf[..n].copy_from_slice(&rb.data[rb.pos..rb.pos + n]);
            rb.pos += n;
            rb.consumed(n);
            // A short answer is legal for every caller here -- `read(n)` loops
            // and the rest are arrival-shaped -- so the peek is handed back
            // on its own rather than topped up with a second syscall.
            return Ok(n);
        }
    }
    blocking_read(f, buf)
}

/// Park until `fd` is ready for `events`, the way CRuby's `rb_io_wait_readable`
/// / `rb_io_wait_writable` do. No timeout -- the caller asked for a BLOCKING
/// operation, and readiness is the only thing it is waiting on.
pub(super) fn wait_ready(f: &std::fs::File, events: libc::c_short) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let mut pfd = libc::pollfd {
        fd: f.as_raw_fd(),
        events,
        revents: 0,
    };
    loop {
        // SAFETY: one initialized `pollfd` describing a descriptor this `File`
        // owns and keeps alive across the call.
        if unsafe { libc::poll(&mut pfd, 1, -1) } >= 0 {
            return Ok(());
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// One `read(2)` with BLOCKING semantics over a descriptor that may carry
/// `O_NONBLOCK` -- which both ends of an `IO.pipe` do, exactly as CRuby marks
/// its own. `EAGAIN` on such a descriptor means "nothing yet", not an error,
/// so this parks in `poll(2)` and retries rather than surfacing it; `EINTR` is
/// the ordinary retry. Every blocking read row goes through here, because a
/// user's own `io.nonblock = true` must not change what `#read` means either.
pub(super) fn blocking_read(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::ErrorKind::{Interrupted, WouldBlock};
    loop {
        match std::io::Read::read(f, buf) {
            Err(e) if e.kind() == Interrupted => continue,
            Err(e) if e.kind() == WouldBlock => wait_ready(f, libc::POLLIN)?,
            other => return other,
        }
    }
}

pub(super) fn io_read_val(
    recv: &RubyValue,
    args: &[RubyValue],
    _blk: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    // A read from STDIN reads the real one; anything else needs a file.
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut buf = String::new();
        crate::gvl::without_gvl(|| std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf))
            .map_err(|e| crate::builtins::file::raise_errno(&e, "read", "<STDIN>"))?;
        return Ok(RubyValue::Str(crate::collections::string_new(buf)));
    }
    let n = match args.first() {
        None | Some(RubyValue::Nil) => None,
        Some(v) => match convert::to_index(v)? {
            i if i >= 0 => Some(i as usize),
            i => return Err(arg_error!("negative length {i} given")),
        },
    };
    // The encoding a WHOLE read tags its bytes with: the handle's external one
    // when it has been set (`File.open(path, "rb")`, an `encoding:` option, a
    // BOM), else UTF-8. UTF-8 unconditionally would mis-tag a binary
    // handle's bytes.
    let read_enc = as_rio(recv)
        .and_then(|io| io.encodings.lock().0)
        .unwrap_or(crate::encoding::UTF_8);
    check_readable(recv)?;
    with_file(recv, |f, path| {
        // A socket peer that closes with unread data sends RST, so a read can
        // return ECONNRESET AFTER delivering the bytes already buffered; CRuby
        // keeps that data and treats the reset as EOF. It never arises for a
        // regular file, so treating it as end here is harmless off a socket.
        // (EINTR/EAGAIN retries live in `blocking_read`.)
        use std::io::ErrorKind::ConnectionReset;
        match n {
            None => {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    match peeked_read(as_rio(recv), f, &mut chunk) {
                        Ok(0) => break,
                        Ok(k) => buf.extend_from_slice(&chunk[..k]),
                        Err(e) if e.kind() == ConnectionReset => break,
                        Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", path)),
                    }
                }
                // TAG the bytes, never re-encode them. Decoding a read as
                // UTF-8 replaced every non-UTF-8 byte with U+FFFD, so a file
                // read through `IO#read` (as opposed to `File.binread`, which
                // was already byte-faithful) came back corrupted -- see the
                // same rule on `write_rio` above.
                Ok(RubyValue::Str(crate::string_from_bytes(buf, read_enc)))
            }
            Some(n) => {
                let mut buf = read_buffer(n)?;
                let mut got = 0;
                // `read` can answer short without being at EOF; loop until
                // the request is filled or the file genuinely ends.
                while got < n {
                    match peeked_read(as_rio(recv), f, &mut buf[got..]) {
                        Ok(0) => break,
                        Ok(k) => got += k,
                        Err(e) if e.kind() == ConnectionReset => break,
                        Err(e) => return Err(crate::builtins::file::raise_errno(&e, "read", path)),
                    }
                }
                buf.truncate(got);
                // EOF + a lengthed read is nil, NOT "" -- the asymmetry a
                // `while chunk = f.read(n)` loop relies on to terminate.
                if got == 0 && n > 0 {
                    return Ok(RubyValue::Nil);
                }
                // A LENGTHED read is binary in CRuby -- a byte count can land
                // mid-character, so there is nothing else it could honestly be.
                Ok(RubyValue::Str(crate::string_from_bytes(
                    buf,
                    crate::encoding::ASCII_8BIT,
                )))
            }
        }
    })
}

/// How `gets`/`readline`/`each_line`/`readlines` split their input: the line
/// separator (`None` = slurp the whole rest, i.e. `gets(nil)`), an optional
/// byte limit, and whether to strip the terminator (`chomp:`).
pub(crate) struct LineOpts {
    pub sep: Option<Vec<u8>>,
    pub limit: Option<usize>,
    pub chomp: bool,
}

/// Parse the shared `(sep = $/, limit = nil, chomp: false)` argument shape.
/// A leading Integer is the limit (separator stays `"\n"`); a leading String
/// is the separator, with an Integer that follows as the limit; a leading nil
/// slurps. The trailing keyword Hash carries `chomp:`.
pub(crate) fn line_opts(args: &[RubyValue]) -> LineOpts {
    let mut sep: Option<Vec<u8>> = Some(b"\n".to_vec());
    let mut limit = None;
    let mut chomp = false;
    let mut positional = args;
    if let Some(RubyValue::Hash(h)) = args.last() {
        chomp = crate::collections::hash_get(h, &RubyValue::Symbol(crate::symbol::wk::chomp()))
            .truthy();
        positional = &args[..args.len() - 1];
    }
    match positional.first() {
        Some(RubyValue::Int(n)) => limit = Some((*n).max(0) as usize),
        Some(RubyValue::Nil) => sep = None,
        Some(RubyValue::Str(s)) => sep = Some(s.lock().bytes().to_vec()),
        _ => {}
    }
    if let Some(RubyValue::Int(n)) = positional.get(1) {
        limit = Some((*n).max(0) as usize);
    }
    LineOpts { sep, limit, chomp }
}

/// Read the next line's bytes: up to and including the separator, or `limit`
/// bytes, or EOF. An empty result means EOF. Byte-at-a-time so the position
/// lands exactly after the line (a buffered read would desync `tell`).
pub(super) fn read_line_bytes(
    io: &RIo,
    f: &mut std::fs::File,
    opts: &LineOpts,
) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        if let Some(lim) = opts.limit
            && out.len() >= lim
        {
            break;
        }
        match buffered_byte(io, f)? {
            None => break,
            Some(b) => {
                out.push(b);
                if let Some(s) = &opts.sep
                    && !s.is_empty()
                    && out.ends_with(s)
                {
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// Turn a line's bytes into the String `gets` answers, honoring `chomp:`.
pub(super) fn line_string(mut bytes: Vec<u8>, opts: &LineOpts) -> RubyValue {
    chomp_line(&mut bytes, opts);
    RubyValue::Str(crate::collections::string_new(
        String::from_utf8_lossy(&bytes).into_owned(),
    ))
}

/// `chomp:`'s rule, on the raw bytes so an encoding-carrying caller
/// (`StringIO`) can apply it without going through UTF-8: strip one trailing
/// custom separator, then any trailing `"\r\n"`/`"\n"`/`"\r"`.
pub(crate) fn chomp_line(bytes: &mut Vec<u8>, opts: &LineOpts) {
    if !opts.chomp {
        return;
    }
    if let Some(sep) = &opts.sep
        && !sep.is_empty()
        && bytes.ends_with(sep)
    {
        bytes.truncate(bytes.len() - sep.len());
    }
    while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        bytes.pop();
    }
}

pub(super) fn bump_lineno(recv: &RubyValue) {
    if let Some(io) = as_rio(recv) {
        io.lineno.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Read the next whole UTF-8 char from `f` (1-4 bytes by the lead byte), or
/// `None` at EOF.
pub(super) fn read_one_char(io: &RIo, f: &mut std::fs::File) -> std::io::Result<Option<String>> {
    let Some(b0) = buffered_byte(io, f)? else {
        return Ok(None);
    };
    let n = if b0 < 0x80 {
        1
    } else if b0 >> 5 == 0b110 {
        2
    } else if b0 >> 4 == 0b1110 {
        3
    } else if b0 >> 3 == 0b11110 {
        4
    } else {
        1
    };
    let mut buf = vec![b0];
    for _ in 1..n {
        let Some(b) = buffered_byte(io, f)? else {
            break;
        };
        buf.push(b);
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// An integer argument (`pread`/`pwrite` counts, `fcntl`/`chmod` operands)
/// through the `to_int` protocol -- unlike the offset sites, a nil here is
/// the generic "of nil into Integer" (oracle-verified).
pub(crate) fn int_of(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion of nil into Integer")),
        v => convert::to_index(v),
    }
}

/// A byte-offset argument (`seek`/`sysseek`/`pos=`/`truncate`, `pread`'s
/// offset): CRuby's NUM2OFFT, whose nil TypeError is the bare
/// "no implicit conversion from nil" (no "to integer" -- oracle-verified).
pub(crate) fn offset_of(v: &RubyValue) -> Result<i64, Signal> {
    match v {
        RubyValue::Nil => Err(type_error!("no implicit conversion from nil")),
        // A String gets the same bare shape, lowercased -- `seek("5")`,
        // `pread(2, "1")` and `copy_stream(a, b, "5")` all say "no implicit
        // conversion from string" where a Symbol gets the ordinary "of
        // Symbol into Integer". Oracle-verified; it is `rb_num2off`'s own
        // split, not a per-method message.
        RubyValue::Str(_) => Err(type_error!("no implicit conversion from string")),
        v => convert::to_index(v),
    }
}

/// `seek`/`sysseek`'s whence: the SET/CUR/END symbols map to their
/// constants (CRuby's interpret_seek_whence); anything else -- unknown
/// symbols included -- goes through NUM2LONG (oracle: `seek(0, :BAD)` is
/// "no implicit conversion of Symbol into Integer").
pub(super) fn whence_of(v: &RubyValue) -> Result<i64, Signal> {
    if let RubyValue::Symbol(s) = v {
        match s.name().as_str() {
            "SET" => return Ok(0),
            "CUR" => return Ok(1),
            "END" => return Ok(2),
            _ => {}
        }
    }
    convert::to_index(v)
}

/// `#gets`'s value, shared with the rows that drain through it
/// (`readline`, `readlines`, `each_line`).
pub(super) fn gets_value(recv: &RubyValue, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    check_readable(recv)?;
    let opts = line_opts(args);
    if matches!(stream_of(recv), Some(StdStream::Stdin)) {
        let mut line = String::new();
        let n = crate::gvl::without_gvl(|| {
            std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut line)
        })
        .map_err(|e| crate::builtins::file::raise_errno(&e, "gets", "<STDIN>"))?;
        if n == 0 {
            return Ok(RubyValue::Nil);
        }
        bump_lineno(recv);
        return Ok(line_string(line.into_bytes(), &opts));
    }
    let line = with_buffered_file(recv, |io, f, path| {
        read_line_bytes(io, f, &opts)
            .map_err(|e| crate::builtins::file::raise_errno(&e, "gets", path))
    })?;
    if line.is_empty() {
        return Ok(RubyValue::Nil);
    }
    bump_lineno(recv);
    Ok(line_string(line, &opts))
}

/// `#getc`'s value -- `#readchar` is this plus an EOF raise.
pub(super) fn getc_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    let ch = with_buffered_file(recv, |io, f, path| {
        read_one_char(io, f).map_err(|e| crate::builtins::file::raise_errno(&e, "getc", path))
    })?;
    Ok(match ch {
        Some(s) => RubyValue::Str(crate::collections::string_new(s)),
        None => RubyValue::Nil,
    })
}

/// `#getbyte`'s value -- `#readbyte` is this plus an EOF raise.
pub(super) fn getbyte_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    // A byte pushed back with `#ungetbyte` rides in the read buffer, so it
    // arrives through the ordinary path below rather than a stack of its own.
    let b = with_buffered_file(recv, |io, f, path| {
        buffered_byte(io, f).map_err(|e| crate::builtins::file::raise_errno(&e, "getbyte", path))
    })?;
    Ok(match b {
        Some(byte) => RubyValue::Int(byte as i64),
        None => RubyValue::Nil,
    })
}

/// An `fstat(2)` snapshot of the open descriptor, as a `File::Stat` --
/// what `#stat`, `#mtime` and `#size` all read.
pub(crate) fn stat_value(recv: &RubyValue) -> Result<RubyValue, Signal> {
    use std::os::fd::AsRawFd;
    with_file(recv, |f, _path| {
        crate::builtins::stat::stat_from_fd(f.as_raw_fd())
    })
}
