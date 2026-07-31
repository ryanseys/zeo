//! `Zlib::GzipWriter` -- an IO-shaped gzip compressor.
//!
//! Every write goes straight through to the underlying IO, so a caller that
//! never closes the writer still gets a valid prefix on disk; `#close` adds
//! the deflate tail and the footer that make it a complete member.

use super::gzip::{Direction, RGzipFile, gz_error, gz_of, open};
use crate::builtins::convert;
use crate::dispatch::send_value;
use crate::{RubyValue, Signal, Symbol};
use flate2::FlushCompress;
use std::sync::Arc;
use zeo_macros::ruby_class;

ruby_class! {
    GzipWriter = zeo_abi::ZLIB_GZIP_WRITER_CLASS < zeo_abi::ZLIB_GZIP_FILE_CLASS;

    // `GzipWriter.new(io, level = nil, strategy = nil)`.
    def self."new" arity -1 (_recv, arg1, arg2?, _arg3?) {
        Ok(writer((*arg1).clone(), super::level_of(arg2), false))
    }

    // `GzipWriter.open(path, level = nil) { |gz| … }` -- opens the file
    // itself, so `#close` closes it, and closes it even if the block raises.
    def self."open" arity -1 (_recv, arg1, arg2?, _arg3?, &block) {
        let file = send_value(
            &RubyValue::Class(zeo_abi::FILE_CLASS),
            Symbol::intern("new"),
            &[(*arg1).clone(), RubyValue::Str(crate::string_new("wb".to_string()))],
            None,
        )?;
        let gz = writer(file, super::level_of(arg2), true);
        with_block(gz, block)
    }

    // `GzipWriter.wrap(io) { |gz| … }` -- same, over an IO the caller owns.
    def self."wrap" arity -1 (_recv, arg1, arg2?, _arg3?, &block) {
        let gz = writer((*arg1).clone(), super::level_of(arg2), false);
        with_block(gz, block)
    }

    // `#write` answers how many bytes went IN, as `IO#write` does -- and like
    // it, accepts any number of arguments, zero included.
    def "write" (recv, *args, &_block) {
        let mut total = 0;
        for arg in args {
            total += write_bytes(recv, &to_bytes(arg)?)?;
        }
        Ok(RubyValue::Int(total))
    }
    def "<<" (recv, other) {
        write_bytes(recv, &to_bytes(other)?)?;
        Ok(recv.clone())
    }
    def "print" (recv, *args, &_block) {
        for arg in args {
            write_bytes(recv, &to_bytes(arg)?)?;
        }
        Ok(RubyValue::Nil)
    }
    def "printf" (recv, *args, &_block) {
        let Some(fmt) = args.first() else {
            return Err(crate::builtins::arg_error!("wrong number of arguments (given 0, expected 1+)"));
        };
        let _ = fmt;
        let fmt = convert::to_rstr(&args[0])?;
        let template = fmt.lock().to_utf8_lossy().to_string();
        let text = crate::builtins::format::sprintf(&template, &args[1..])?;
        write_bytes(recv, text.as_bytes())?;
        Ok(RubyValue::Nil)
    }
    def "putc" (recv, arg) {
        let byte = match arg {
            RubyValue::Int(n) => vec![*n as u8],
            other => to_bytes(other)?.into_iter().take(1).collect(),
        };
        write_bytes(recv, &byte)?;
        Ok((*arg).clone())
    }
    // `#puts` follows `Kernel#puts`: no arguments is a bare newline, an Array
    // is flattened, and a line that already ends in one is not given another.
    def "puts" (recv, *args, &_block) {
        if args.is_empty() {
            write_bytes(recv, b"\n")?;
        }
        for arg in args {
            for line in puts_lines(arg)? {
                write_bytes(recv, &line)?;
                if !line.ends_with(b"\n") {
                    write_bytes(recv, b"\n")?;
                }
            }
        }
        Ok(RubyValue::Nil)
    }

    // How many UNCOMPRESSED bytes have been written -- CRuby's `pos` for a
    // writer, and the value that ends up in the footer's length field.
    def "pos" (recv) {
        Ok(RubyValue::Int(open(recv)?.position()?))
    }
    def "tell" (recv) {
        Ok(RubyValue::Int(open(recv)?.position()?))
    }

    // `#flush(flush = SYNC_FLUSH)` -- push what has been compressed so far
    // through to the IO without ending the member.
    def "flush" (recv, arg?) {
        let flush = match arg {
            None => FlushCompress::Sync,
            other => super::codec::flush_of(other)?,
        };
        let st = &mut *open(recv)?;
        let header = st.header.clone();
        let io = st.io.clone();
        let Direction::Write(w) = &mut st.dir else {
            return Err(gz_error("not opened for writing"));
        };
        w.write(&io, &header, &[], flush)?;
        Ok(recv.clone())
    }

    // The header fields, settable only until the header goes out -- which the
    // first `write` does. CRuby raises rather than silently dropping a late
    // assignment, because the value would never reach the file.
    def "mtime=" (recv, arg) {
        let secs = match arg {
            RubyValue::Int(n) => *n,
            other => convert::to_index(&send_value(other, Symbol::intern("to_i"), &[], None)?)?,
        };
        settable(recv)?.header.mtime = secs as u32;
        Ok((*arg).clone())
    }
    def "orig_name=" (recv, arg) {
        settable(recv)?.header.orig_name = Some(header_text(arg)?);
        Ok((*arg).clone())
    }
    def "comment=" (recv, arg) {
        settable(recv)?.header.comment = Some(header_text(arg)?);
        Ok((*arg).clone())
    }
}

fn writer(io: RubyValue, level: flate2::Compression, owns_io: bool) -> RubyValue {
    RubyValue::Object(Arc::new(RGzipFile::writer(io, level, owns_io)))
}

/// `open`/`wrap`'s block form: yield the writer, close it however the block
/// leaves -- a raise through the block must still finish the member, or the
/// file on disk is a header with no footer.
fn with_block(gz: RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(gz);
    };
    let result = p.call(std::slice::from_ref(&gz));
    let closed = gz_of(&gz).state.lock().shut_down(true);
    match result {
        // The block's own failure wins: the close error (if any) happened
        // while unwinding and would hide the real cause.
        Err(e) => Err(e),
        Ok(value) => closed.map(|_| value),
    }
}

/// The state of a writer whose header has NOT gone out yet.
fn settable(
    recv: &RubyValue,
) -> Result<parking_lot::MutexGuard<'_, super::gzip::GzipState>, Signal> {
    let st = open(recv)?;
    match &st.dir {
        Direction::Write(w) if w.header_written => Err(gz_error("header is already written")),
        Direction::Write(_) => Ok(st),
        Direction::Read(_) => Err(gz_error("not opened for writing")),
    }
}

/// Compress `data` into the IO, keeping the footer's two fields up to date.
fn write_bytes(recv: &RubyValue, data: &[u8]) -> Result<i64, Signal> {
    let st = &mut *open(recv)?;
    st.crc = super::crc32(data, st.crc);
    st.size = st.size.wrapping_add(data.len() as u32);
    let header = st.header.clone();
    let io = st.io.clone();
    let Direction::Write(w) = &mut st.dir else {
        return Err(gz_error("not opened for writing"));
    };
    w.write(&io, &header, data, FlushCompress::None)?;
    Ok(data.len() as i64)
}

/// A written value's bytes: a String as-is, anything else through `to_s`.
fn to_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    match v {
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        other => {
            let s = send_value(other, Symbol::intern("to_s"), &[], None)?;
            Ok(convert::to_rstr(&s)?.lock().bytes().to_vec())
        }
    }
}

/// A gzip header string is NUL-terminated on the wire, so it cannot contain
/// one -- CRuby rejects that rather than writing a header that reads back
/// truncated.
fn header_text(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    let bytes = to_bytes(v)?;
    if bytes.contains(&0) {
        return Err(gz_error("string contains null byte"));
    }
    Ok(bytes)
}

/// One `puts` argument as the lines it contributes. An Array is flattened (a
/// nested empty one contributes a bare newline), everything else is one line.
fn puts_lines(v: &RubyValue) -> Result<Vec<Vec<u8>>, Signal> {
    let RubyValue::Array(a) = v else {
        return Ok(vec![to_bytes(v)?]);
    };
    let elems = a.lock().to_vec();
    if elems.is_empty() {
        return Ok(vec![Vec::new()]);
    }
    let mut out = Vec::new();
    for e in &elems {
        out.extend(puts_lines(e)?);
    }
    Ok(out)
}
