//! `Zlib::GzipReader` -- an IO-shaped gzip decompressor, `Enumerable` over its
//! lines.
//!
//! The member's header is read by `new`, which is why a non-gzip stream fails
//! there rather than at the first `read`. Everything after that follows the
//! one rule in [`super::gzip`]'s header: fill, then let `check_when_drained`
//! decide whether the answer is data or the end of the member.

use super::gzip::{RGzipFile, gz_error, gz_of, open};
use crate::builtins::convert;
use crate::dispatch::send_value;
use crate::{RubyValue, Signal, Symbol};
use std::sync::Arc;
use zeo_macros::ruby_class;

ruby_class! {
    GzipReader = zeo_abi::ZLIB_GZIP_READER_CLASS < zeo_abi::ZLIB_GZIP_FILE_CLASS;

    def self."new" arity -1 (_recv, arg1, _arg2?) {
        reader((*arg1).clone(), false)
    }

    // `GzipReader.open(path) { |gz| … }` -- opens the file itself, so `#close`
    // closes it, and closes it even if the block raises.
    def self."open" arity -1 (_recv, arg1, _arg2?, &block) {
        let file = send_value(
            &RubyValue::Class(zeo_abi::FILE_CLASS),
            Symbol::intern("new"),
            &[(*arg1).clone(), RubyValue::Str(crate::string_new("rb".to_string()))],
            None,
        )?;
        let gz = reader(file, true)?;
        with_block(gz, block)
    }
    def self."wrap" arity -1 (_recv, arg1, _arg2?, &block) {
        let gz = reader((*arg1).clone(), false)?;
        with_block(gz, block)
    }

    // `GzipReader.zcat(io)` -- every member in the stream, concatenated.
    // Where `read` stops at the first footer, this picks the next member up
    // out of what followed it.
    def self."zcat" arity -1 (_recv, arg1, _arg2?, &block) {
        let mut all = Vec::new();
        let mut rest = read_everything(arg1)?;
        loop {
            let (bytes, _, unused) = super::codec::gunzip_bytes(&rest)?;
            all.extend_from_slice(&bytes);
            if unused.is_empty() { break; }
            rest = unused;
        }
        match block {
            Some(RubyValue::Proc(p)) => { p.call(&[super::text_str(all)])?; Ok(RubyValue::Nil) }
            _ => Ok(super::text_str(all)),
        }
    }

    // `#read` with no length answers the rest of the member as TEXT (the
    // external encoding); with one it answers raw bytes, and nil at the end.
    // That split is CRuby's, not an oversight.
    def "read" (recv, arg1?, arg2?) {
        let st = &mut *open(recv)?;
        match arg1 {
            None | Some(RubyValue::Nil) => {
                let bytes = st.read_all()?.unwrap_or_default();
                Ok(into_buffer(arg2, super::text_str(bytes)))
            }
            Some(v) => {
                let want = convert::to_index(v)?.max(0) as usize;
                // `read(0)` is "" without touching the stream, as IO's is.
                if want == 0 {
                    return Ok(super::bin_str(Vec::new()));
                }
                match st.read_n(want)? {
                    None => Ok(RubyValue::Nil),
                    Some(bytes) => Ok(into_buffer(arg2, super::bin_str(bytes))),
                }
            }
        }
    }
    // `#readpartial` hands back whatever is already decoded rather than
    // waiting for `len` bytes, and reports the end of the member as EOFError.
    def "readpartial" arity -1 (recv, arg1, arg2?) {
        let want = convert::to_index(arg1)?.max(0) as usize;
        let st = &mut *open(recv)?;
        match st.read_partial(want)? {
            None => Err(crate::builtins::eof_error!("end of file reached")),
            Some(bytes) => Ok(into_buffer(arg2, super::bin_str(bytes))),
        }
    }

    // The line surface. `gets` answers nil at the end, `readline` raises.
    def "gets" (recv, arg1?, _arg2?) {
        Ok(read_line(recv, arg1)?.unwrap_or(RubyValue::Nil))
    }
    def "readline" (recv, arg1?, _arg2?) {
        read_line(recv, arg1)?
            .ok_or_else(|| crate::builtins::eof_error!("end of file reached"))
    }
    def "readlines" (recv, arg1?, _arg2?) {
        let mut lines = Vec::new();
        while let Some(line) = read_line(recv, arg1)? {
            lines.push(line);
        }
        Ok(RubyValue::Array(crate::array_new(lines)))
    }
    // `each`/`each_line` are the same method, and are what `Enumerable` walks.
    def "each" (recv, arg1?, _arg2?, &block) {
        each_line(recv, arg1, block)
    }
    def "each_line" (recv, arg1?, _arg2?, &block) {
        each_line(recv, arg1, block)
    }

    // Single characters and bytes. `getc`/`getbyte` answer nil at the end,
    // `readchar`/`readbyte` raise.
    def "getc" (recv) {
        Ok(next_char(recv)?.unwrap_or(RubyValue::Nil))
    }
    def "readchar" (recv) {
        next_char(recv)?.ok_or_else(|| crate::builtins::eof_error!("end of file reached"))
    }
    def "getbyte" (recv) {
        Ok(next_byte(recv)?.unwrap_or(RubyValue::Nil))
    }
    def "readbyte" (recv) {
        next_byte(recv)?.ok_or_else(|| crate::builtins::eof_error!("end of file reached"))
    }
    def "each_byte" (recv, &block) {
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "each_byte", &[]));
        };
        while let Some(byte) = next_byte(recv)? {
            p.call(&[byte])?;
        }
        Ok(recv.clone())
    }
    def "each_char" (recv, &block) {
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(crate::builtins::enumerator::enumerator_for(recv, "each_char", &[]));
        };
        while let Some(ch) = next_char(recv)? {
            p.call(&[ch])?;
        }
        Ok(recv.clone())
    }
    def "ungetc" (recv, arg) {
        open(recv)?.unread(&to_bytes(arg)?)?;
        Ok(RubyValue::Nil)
    }
    def "ungetbyte" (recv, arg) {
        let byte = match arg {
            RubyValue::Int(n) => vec![*n as u8],
            other => to_bytes(other)?,
        };
        open(recv)?.unread(&byte)?;
        Ok(RubyValue::Nil)
    }

    // Position and state.
    def "pos" (recv) {
        Ok(RubyValue::Int(open(recv)?.position()?))
    }
    def "tell" (recv) {
        Ok(RubyValue::Int(open(recv)?.position()?))
    }
    def "eof?" (recv) {
        Ok(RubyValue::Bool(open(recv)?.at_eof()?))
    }
    def "eof" (recv) {
        Ok(RubyValue::Bool(open(recv)?.at_eof()?))
    }
    def "lineno" (recv) {
        Ok(RubyValue::Int(open(recv)?.read_state()?.lineno))
    }
    def "lineno=" (recv, arg) {
        open(recv)?.read_state()?.lineno = convert::to_index(arg)?;
        Ok((*arg).clone())
    }
    // Bytes that followed the member's footer -- nil until it has been read.
    def "unused" (recv) {
        Ok(match open(recv)?.unused()? {
            None => RubyValue::Nil,
            Some(bytes) => super::bin_str(bytes),
        })
    }
    // What `read`/`gets` tag their answers with -- the process's external
    // encoding, as CRuby's does.
    def "external_encoding" (recv) {
        drop(open(recv)?);
        Ok(crate::builtins::encoding::encoding_value(crate::encoding::UTF_8))
    }

    // Start the member over. Only a seekable IO can do this, since it means
    // rewinding the compressed stream and re-reading the header.
    def "rewind" (recv) {
        open(recv)?.rewind()?;
        Ok(RubyValue::Int(0))
    }
}

/// Build a reader and read its header, which is where a stream that isn't
/// gzip at all is reported -- CRuby raises from `new` too.
fn reader(io: RubyValue, owns_io: bool) -> Result<RubyValue, Signal> {
    let gz = RGzipFile::reader(io, owns_io);
    {
        let st = &mut *gz.state.lock();
        let io = st.io.clone();
        st.header = st.read_state()?.read_header(&io)?;
    }
    Ok(RubyValue::Object(Arc::new(gz)))
}

/// `open`/`wrap`'s block form: yield the reader and close it however the
/// block leaves.
fn with_block(gz: RubyValue, block: Option<RubyValue>) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        return Ok(gz);
    };
    let result = p.call(std::slice::from_ref(&gz));
    let closed = gz_of(&gz).state.lock().shut_down(true);
    match result {
        Err(e) => Err(e),
        Ok(value) => closed.map(|_| value),
    }
}

/// One line, tagged with the external encoding. A nil separator means "the
/// whole rest of the member"; the default is `$/`.
fn read_line(recv: &RubyValue, sep: Option<&RubyValue>) -> Result<Option<RubyValue>, Signal> {
    let sep = match sep {
        None => Some(b"\n".to_vec()),
        Some(RubyValue::Nil) => None,
        Some(v) => Some(convert::to_rstr(v)?.lock().bytes().to_vec()),
    };
    let st = &mut *open(recv)?;
    Ok(st.read_line(sep.as_deref())?.map(super::text_str))
}

/// `each`/`each_line`. Blockless, this is an Enumerator over the same lines --
/// Ruby's rule for every `each_*`, and what `each_line.to_a` needs.
fn each_line(
    recv: &RubyValue,
    sep: Option<&RubyValue>,
    block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    let Some(RubyValue::Proc(p)) = block else {
        let args: Vec<RubyValue> = sep.cloned().into_iter().collect();
        return Ok(crate::builtins::enumerator::enumerator_for(
            recv,
            "each_line",
            &args,
        ));
    };
    while let Some(line) = read_line(recv, sep)? {
        p.call(&[line])?;
    }
    Ok(recv.clone())
}

/// One character, decoded as UTF-8 -- a multi-byte character may straddle the
/// buffer, so this reads bytes until it has a whole one.
fn next_char(recv: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    let st = &mut *open(recv)?;
    let Some(first) = st.read_n(1)? else {
        return Ok(None);
    };
    let want = utf8_len(first[0]);
    let mut bytes = first;
    while bytes.len() < want {
        match st.read_n(want - bytes.len())? {
            None => break,
            Some(more) => bytes.extend_from_slice(&more),
        }
    }
    Ok(Some(super::text_str(bytes)))
}

fn next_byte(recv: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    let st = &mut *open(recv)?;
    Ok(st
        .read_n(1)?
        .and_then(|b| b.first().map(|&n| RubyValue::Int(i64::from(n)))))
}

/// How many bytes the UTF-8 character starting with `lead` occupies. A
/// continuation or invalid byte stands alone, which is what CRuby's `getc`
/// does with malformed input rather than raising.
fn utf8_len(lead: u8) -> usize {
    match lead {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 1,
    }
}

/// `read`/`readpartial`'s optional output buffer: the bytes are copied into
/// the caller's String and that String is the answer.
fn into_buffer(outbuf: Option<&RubyValue>, value: RubyValue) -> RubyValue {
    let (Some(RubyValue::Str(dst)), RubyValue::Str(src)) = (outbuf, &value) else {
        return value;
    };
    let bytes = src.lock().bytes().to_vec();
    let enc = src.lock().encoding();
    dst.lock().replace_bytes(bytes, enc);
    outbuf.expect("matched a Some above").clone()
}

fn to_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(convert::to_rstr(v)?.lock().bytes().to_vec())
}

/// Slurp an IO for `zcat`, which has to see every member and so cannot stream.
fn read_everything(io: &RubyValue) -> Result<Vec<u8>, Signal> {
    match send_value(io, Symbol::intern("read"), &[], None)? {
        RubyValue::Str(s) => Ok(s.lock().bytes().to_vec()),
        RubyValue::Nil => Ok(Vec::new()),
        _ => Err(gz_error("not opened for reading")),
    }
}
