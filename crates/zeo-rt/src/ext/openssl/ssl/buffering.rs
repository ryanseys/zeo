//! `OpenSSL::Buffering` -- the buffered IO surface CRuby mixes into
//! `SSLSocket`.
//!
//! Every method here is written against the receiver's `sysread`, `syswrite`
//! and `sysclose`, exactly as upstream's `buffering.rb` is, so the module
//! carries no knowledge of TLS and any object supplying those three can mix it
//! in. Read state lives in the receiver's `@rbuffer`/`@eof` ivars, which is
//! what lets `ungetc` push a character back.
//!
//! One divergence, inherited from the socket underneath: `read_nonblock` and
//! `write_nonblock` work a BLOCKING descriptor, so they never answer
//! `:wait_readable`.

use crate::builtins::{block_or_enum, convert, eof_error};
use crate::dispatch::send_value_in;
use crate::ext::openssl::{bin_str, str_bytes};
use crate::signal::Signal;
use crate::{RubyValue, Symbol};
use zeo_macros::ruby_module;

/// CRuby's `Buffering::BLOCK_SIZE` -- one `sysread` request.
const BLOCK_SIZE: i64 = 1024 * 16;

fn send(recv: &RubyValue, name: &str, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    send_value_in(0, recv, Symbol::intern(name), args, None)
}

/// The receiver's ivar table. Present for every includer -- a native object
/// without one cannot hold the read buffer, so say so rather than silently
/// dropping bytes.
fn ivar_get(recv: &RubyValue, name: &str) -> RubyValue {
    match recv {
        RubyValue::Object(o) => o.ivar_get_named(name).unwrap_or(RubyValue::Nil),
        _ => RubyValue::Nil,
    }
}

fn ivar_set(recv: &RubyValue, name: &str, v: RubyValue) {
    if let RubyValue::Object(o) = recv {
        o.ivar_set_named(name, v);
    }
}

fn rbuffer(recv: &RubyValue) -> Result<Vec<u8>, Signal> {
    match ivar_get(recv, "rbuffer") {
        RubyValue::Nil => Ok(Vec::new()),
        v => str_bytes(&v),
    }
}

fn set_rbuffer(recv: &RubyValue, bytes: Vec<u8>) {
    ivar_set(recv, "rbuffer", bin_str(bytes));
}

/// Whether `sig` is the `EOFError` a `sysread` raises at the end of the
/// stream -- the one signal the fill loop absorbs, as upstream's
/// `rescue EOFError` does.
fn is_eof(sig: &Signal) -> bool {
    let Signal::Raise(exc) = sig else {
        return false;
    };
    let Some(eof) = crate::dispatch::class_id_by_name("EOFError") else {
        return false;
    };
    crate::dispatch::ancestors_of_value(exc.class_id()).contains(&eof)
}

/// One `sysread` into the buffer. `Ok(false)` at the end of the stream, which
/// also latches `@eof`.
fn fill(recv: &RubyValue) -> Result<bool, Signal> {
    if ivar_get(recv, "eof").truthy() {
        return Ok(false);
    }
    match send(recv, "sysread", &[RubyValue::Int(BLOCK_SIZE)]) {
        Ok(v) => {
            let mut buf = rbuffer(recv)?;
            buf.extend_from_slice(&str_bytes(&v)?);
            set_rbuffer(recv, buf);
            Ok(true)
        }
        Err(sig) if is_eof(&sig) => {
            ivar_set(recv, "eof", RubyValue::Bool(true));
            Ok(false)
        }
        Err(sig) => Err(sig),
    }
}

/// Take up to `want` bytes, reading more only when the buffer runs dry.
/// `None` for "want" reads everything to the end of the stream.
fn take(recv: &RubyValue, want: Option<usize>) -> Result<Vec<u8>, Signal> {
    let mut buf = rbuffer(recv)?;
    match want {
        None => {
            while fill(recv)? {}
            buf = rbuffer(recv)?;
            set_rbuffer(recv, Vec::new());
            Ok(buf)
        }
        Some(want) => {
            while buf.len() < want {
                if !fill(recv)? {
                    break;
                }
                buf = rbuffer(recv)?;
            }
            let rest = buf.split_off(want.min(buf.len()));
            set_rbuffer(recv, rest);
            Ok(buf)
        }
    }
}

/// Whether the buffer is empty AND the stream has ended.
fn at_eof(recv: &RubyValue) -> Result<bool, Signal> {
    if !rbuffer(recv)?.is_empty() {
        return Ok(false);
    }
    Ok(!fill(recv)?)
}

/// One line, up to and including `sep`. `None` at the end of the stream.
fn read_line(recv: &RubyValue, sep: u8) -> Result<Option<Vec<u8>>, Signal> {
    let mut out = Vec::new();
    loop {
        let buf = rbuffer(recv)?;
        if let Some(at) = buf.iter().position(|&b| b == sep) {
            let mut line = buf;
            let rest = line.split_off(at + 1);
            set_rbuffer(recv, rest);
            out.extend_from_slice(&line);
            return Ok(Some(out));
        }
        out.extend_from_slice(&buf);
        set_rbuffer(recv, Vec::new());
        if !fill(recv)? {
            return Ok(if out.is_empty() { None } else { Some(out) });
        }
    }
}

/// The `sep` argument `gets`/`each_line`/`readlines` share. Only its LAST byte
/// matters here, which is the same simplification the unbuffered reader made.
fn separator(arg: Option<&RubyValue>) -> Result<u8, Signal> {
    Ok(match arg {
        None | Some(RubyValue::Nil) => b'\n',
        Some(v) => *str_bytes(v)?.last().unwrap_or(&b'\n'),
    })
}

/// `syswrite` every argument, answering the total byte count.
fn write_args(recv: &RubyValue, args: &[RubyValue]) -> Result<i64, Signal> {
    let mut total = 0i64;
    for arg in args {
        // A trailing kwargs hash is `exception: false`, not data.
        if matches!(arg, RubyValue::Hash(_)) {
            continue;
        }
        let data = str_bytes(arg)?;
        total += data.len() as i64;
        send(recv, "syswrite", std::slice::from_ref(arg))?;
    }
    Ok(total)
}

ruby_module! {
    Buffering = zeo_abi::OPENSSL_BUFFERING_MODULE;
    include zeo_abi::ENUMERABLE_CLASS;

    const BLOCK_SIZE = RubyValue::Int(BLOCK_SIZE);

    // Writes go straight to the session, so sync is always true; the setter
    // records what it was given without changing that.
    def "sync" (recv) {
        Ok(match ivar_get(recv, "sync") { RubyValue::Nil => RubyValue::Bool(true), v => v })
    }
    def "sync=" (recv, arg) {
        ivar_set(recv, "sync", (*arg).clone());
        Ok((*arg).clone())
    }

    def "write" | "write_nonblock" arity -2 (recv, *args, &_block) {
        Ok(RubyValue::Int(write_args(recv, args)?))
    }
    def "<<" (recv, s) {
        send(recv, "syswrite", std::slice::from_ref(s))?;
        Ok(recv.clone())
    }
    // `print`/`printf`/`puts` answer nil, unlike `write`'s byte count.
    def "print" (recv, *args, &_block) {
        write_args(recv, args)?;
        Ok(RubyValue::Nil)
    }
    def "printf" (recv, template, *args, &_block) {
        let template = convert::to_rstr(template)?.lock().to_utf8_lossy().into_owned();
        let text = crate::builtins::format::sprintf(&template, args)?;
        send(recv, "syswrite", &[crate::ext::openssl::str(text)])?;
        Ok(RubyValue::Nil)
    }
    def "puts" (recv, *args, &_block) {
        let mut out = Vec::new();
        if args.is_empty() {
            out.push(b'\n');
        }
        for arg in args {
            let mut data = str_bytes(arg)?;
            if !data.ends_with(b"\n") {
                data.push(b'\n');
            }
            out.extend_from_slice(&data);
        }
        send(recv, "syswrite", &[bin_str(out)])?;
        Ok(RubyValue::Nil)
    }
    // Nothing is held back, so there is nothing to push out.
    def "flush" (recv) {
        Ok(recv.clone())
    }

    // `read(len = nil)` -- to the end of the stream without a length, exactly
    // `len` bytes (short at the end) with one, `nil` at the end for a positive
    // length.
    def "read" (recv, arg1?, _arg2?) {
        let want = match arg1 {
            None | Some(RubyValue::Nil) => None,
            Some(v) => Some(convert::to_index(v)? as usize),
        };
        let out = take(recv, want)?;
        if out.is_empty() && want.is_some_and(|w| w > 0) {
            return Ok(RubyValue::Nil);
        }
        Ok(bin_str(out))
    }
    // Whatever is buffered, or one `sysread`'s worth; EOFError at the end.
    def "readpartial" | "read_nonblock" (recv, arg1, _arg2?, _arg3?) {
        let want = match arg1 {
            RubyValue::Nil => BLOCK_SIZE as usize,
            v => convert::to_index(v)? as usize,
        };
        if rbuffer(recv)?.is_empty() && !fill(recv)? && want > 0 {
            return Err(eof_error!("end of file reached"));
        }
        Ok(bin_str(take(recv, Some(want))?))
    }

    def "gets" (recv, arg1?, _arg2?) {
        let sep = separator(arg1)?;
        Ok(match read_line(recv, sep)? {
            Some(line) => bin_str(line),
            None => RubyValue::Nil,
        })
    }
    def "readline" (recv, arg1?, _arg2?) {
        let sep = separator(arg1)?;
        match read_line(recv, sep)? {
            Some(line) => Ok(bin_str(line)),
            None => Err(eof_error!("end of file reached")),
        }
    }
    def "readlines" (recv, arg1?, _arg2?) {
        let sep = separator(arg1)?;
        let mut lines = Vec::new();
        while let Some(line) = read_line(recv, sep)? {
            lines.push(bin_str(line));
        }
        Ok(RubyValue::Array(crate::array_new(lines)))
    }
    def "each" | "each_line" cfunc (recv, sep?, _limit?, &block) {
        let sep = separator(sep)?;
        let p = block_or_enum!(recv, "each_line", __args, block);
        while let Some(line) = read_line(recv, sep)? {
            p.call(&[bin_str(line)])?;
        }
        Ok(recv.clone())
    }

    def "getbyte" (recv) {
        let b = take(recv, Some(1))?;
        Ok(match b.first() {
            Some(&b) => RubyValue::Int(i64::from(b)),
            None => RubyValue::Nil,
        })
    }
    def "readbyte" (recv) {
        let b = take(recv, Some(1))?;
        match b.first() {
            Some(&b) => Ok(RubyValue::Int(i64::from(b))),
            None => Err(eof_error!("end of file reached")),
        }
    }
    // One CHARACTER: the buffer holds bytes, so a multi-byte lead byte pulls
    // in the rest of its sequence before answering.
    def "getc" (recv) {
        Ok(match read_char(recv)? {
            Some(c) => c,
            None => RubyValue::Nil,
        })
    }
    def "readchar" (recv) {
        match read_char(recv)? {
            Some(c) => Ok(c),
            None => Err(eof_error!("end of file reached")),
        }
    }
    def "each_byte" (recv, &block) {
        let p = block_or_enum!(recv, &[], block);
        loop {
            let b = take(recv, Some(1))?;
            let Some(&b) = b.first() else { break };
            p.call(&[RubyValue::Int(i64::from(b))])?;
        }
        Ok(recv.clone())
    }

    // Push a character back onto the front of the read buffer, so the next
    // read sees it again. This is the one method that needs the receiver to
    // hold state of its own.
    def "ungetc" (recv, arg) {
        let mut back = str_bytes(arg)?;
        back.extend_from_slice(&rbuffer(recv)?);
        set_rbuffer(recv, back);
        Ok(RubyValue::Nil)
    }

    def "eof?" | "eof" (recv) {
        Ok(RubyValue::Bool(at_eof(recv)?))
    }

    // Upstream answers whatever `sysclose` did, which for an SSLSocket is nil.
    def "close" (recv) {
        send(recv, "sysclose", &[])
    }
}

/// `getc`/`readchar`'s shared body: one whole UTF-8 character, or `None` at
/// the end of the stream. An invalid lead byte answers that single byte, the
/// way a binary String holds one.
fn read_char(recv: &RubyValue) -> Result<Option<RubyValue>, Signal> {
    let lead = take(recv, Some(1))?;
    let Some(&lead) = lead.first() else {
        return Ok(None);
    };
    let more = match lead {
        0xc0..=0xdf => 1,
        0xe0..=0xef => 2,
        0xf0..=0xf7 => 3,
        _ => 0,
    };
    let mut out = vec![lead];
    out.extend_from_slice(&take(recv, Some(more))?);
    Ok(Some(crate::ext::openssl::str(
        String::from_utf8_lossy(&out).into_owned(),
    )))
}
