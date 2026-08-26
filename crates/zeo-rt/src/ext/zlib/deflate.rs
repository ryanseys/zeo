//! `Zlib::Deflate` -- a compressing [`codec`] stream.

use super::codec::{
    self, Flush, RZStream, Wrap, bytes_of, flush_of, ready, run_and_maybe_detach, stream_error,
    wrap_of, yield_or_return,
};
use crate::RubyValue;
use crate::builtins::{convert, not_impl_error};
use flate2::FlushCompress;
use std::sync::Arc;
use zeo_macros::ruby_class;

ruby_class! {
    Deflate = zeo_abi::ZLIB_DEFLATE_CLASS < zeo_abi::ZLIB_ZSTREAM_CLASS;

    // `Deflate.new(level, window_bits, mem_level, strategy)`. `mem_level` is
    // accepted and ignored -- it sizes zlib's internal tables, which the
    // pure-Rust backend fixes.
    def self."new" (_recv, arg1?, arg2?, _arg3?, arg4?) {
        let level = super::level_of(arg1)?;
        let wrap = match arg2 {
            None | Some(RubyValue::Nil) => Wrap::Zlib,
            Some(v) => wrap_of(convert::to_index(v)?, false)?,
        };
        let strategy = match arg4 {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => convert::to_index(v)?,
        };
        Ok(RubyValue::Object(Arc::new(RZStream::deflating(wrap, level, strategy))))
    }

    // `Deflate.deflate(string, level)` -- a whole stream in one call.
    def self."deflate" cfunc (_recv, string, level?) {
        codec::one_shot_deflate(&bytes_of(Some(string))?, super::level_of(level)?, Wrap::Zlib)
    }

    // `#deflate(string, flush = NO_FLUSH)` -- feed input, take back whatever
    // has been flushed so far (with NO_FLUSH, usually nothing).
    def "deflate" cfunc (recv, string, flush?, &block) {
        let flush = flush_of(flush)?;
        let out = run_and_maybe_detach(recv, &bytes_of(Some(string))?, Flush::Compress(flush), true)?;
        yield_or_return(out, block)
    }
    // `#<<` feeds input and QUEUES the output for the next detaching call.
    def "<<" (recv, string) {
        run_and_maybe_detach(
            recv, &bytes_of(Some(string))?, Flush::Compress(FlushCompress::None), false,
        )
    }
    // `#flush(flush = SYNC_FLUSH)` -- flush without ending the stream.
    def "flush" (recv, arg?, &block) {
        let flush = match arg {
            None => FlushCompress::Sync,
            other => flush_of(other)?,
        };
        let out = run_and_maybe_detach(recv, &[], Flush::Compress(flush), true)?;
        yield_or_return(out, block)
    }

    // `#params(level, strategy)` needs `deflateParams`, which flate2's
    // pure-Rust backend does not compile (`Compress::set_level` is `#[cfg]`'d
    // to the C-zlib and zlib-rs backends). The settings are still recorded, so
    // a caller that reads them back sees what it asked for; what cannot happen
    // is the change taking effect mid-stream. CRuby's own `params` raises
    // `Zlib::StreamError` in both natural call shapes and segfaults in a third
    // (ruby 4.0.5, `rb_deflate_params`), so nothing depends on it working.
    def "params" (recv, level, strategy) {
        let level = super::level_of(Some(level))?;
        let strategy = convert::to_index(strategy)?;
        let st = &mut *ready(recv)?;
        let codec::Codec::Deflate(d) = &mut st.codec else {
            return Err(stream_error());
        };
        d.level = level;
        d.strategy = strategy;
        Err(not_impl_error!(
            "Zlib::Deflate#params needs deflateParams, which zeo's pure-Rust deflate backend does not provide"
        ))
    }

    // A preset dictionary needs `deflateSetDictionary`, likewise absent.
    // Naming the missing capability beats a silent no-op, which would produce
    // a stream that decodes to the wrong bytes rather than failing.
    def "set_dictionary" (_recv, _arg) {
        Err(not_impl_error!(
            "Zlib::Deflate#set_dictionary needs deflateSetDictionary, which zeo's pure-Rust deflate backend does not provide"
        ))
    }
}
