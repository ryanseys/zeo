//! `Zlib::Inflate` -- a decompressing [`codec`] stream.

use super::codec::{
    self, Flush, RZStream, Wrap, bytes_of, run_and_maybe_detach, wrap_of, yield_or_return, zs_of,
};
use crate::RubyValue;
use crate::builtins::{arity, convert, not_impl_error};
use flate2::FlushDecompress;
use std::sync::Arc;
use zeo_macros::ruby_class;

ruby_class! {
    Inflate = zeo_abi::ZLIB_INFLATE_CLASS < zeo_abi::ZLIB_ZSTREAM_CLASS;

    // `Inflate.new(window_bits)`. Unlike `Deflate`, this accepts the +32
    // auto-detect form -- which is how `Net::HTTP` decodes a response body
    // before it has decided whether the encoding was gzip or deflate.
    def self."new" (_recv, arg?) {
        let wrap = match arg {
            None | Some(RubyValue::Nil) => Wrap::Zlib,
            Some(v) => wrap_of(convert::to_index(v)?, true)?,
        };
        Ok(RubyValue::Object(Arc::new(RZStream::inflating(wrap))))
    }

    // `Inflate.inflate(string)` -- a whole stream in one call.
    def self."inflate" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        codec::one_shot_inflate(&bytes_of(args.first())?, Wrap::Zlib)
    }

    // `#inflate(string)` -- feed input, take back what it decompressed to. A
    // nil argument means "no more input", i.e. finish.
    def "inflate" (recv, arg?, &block) {
        if matches!(arg, None | Some(RubyValue::Nil)) {
            // Finishing a stream nothing was ever fed is CRuby's documented
            // way to close out an EMPTY member, and answers "" rather than
            // the buffer error a TRUNCATED one gets.
            let out = if zs_of(recv).state.lock().total_in == 0 {
                super::bin_str(Vec::new())
            } else {
                codec::finish(recv)?
            };
            return yield_or_return(out, block);
        }
        let out = run_and_maybe_detach(
            recv, &bytes_of(arg)?, Flush::Decompress(FlushDecompress::None), true,
        )?;
        yield_or_return(out, block)
    }
    // `#<<` feeds input and QUEUES the output for the next detaching call.
    def "<<" (recv, *args, &_block) {
        arity!(args, 1);
        run_and_maybe_detach(
            recv, &bytes_of(args.first())?, Flush::Decompress(FlushDecompress::None), false,
        )
    }

    // zlib's mid-stream resynchronization and preset dictionaries both need
    // entry points flate2's pure-Rust backend does not compile. `sync_point?`
    // still answers, because `false` is what a caller who never called `sync`
    // would see from CRuby too.
    def "sync" (_recv, _arg) {
        Err(not_impl_error!(
            "Zlib::Inflate#sync needs inflateSync, which zeo's pure-Rust inflate backend does not provide"
        ))
    }
    def "sync_point?" (recv) {
        drop(codec::ready(recv)?);
        Ok(RubyValue::Bool(false))
    }
    def "set_dictionary" (_recv, _arg) {
        Err(not_impl_error!(
            "Zlib::Inflate#set_dictionary needs inflateSetDictionary, which zeo's pure-Rust inflate backend does not provide"
        ))
    }
    def "add_dictionary" (_recv, _arg) {
        Err(not_impl_error!(
            "Zlib::Inflate#add_dictionary needs inflateSetDictionary, which zeo's pure-Rust inflate backend does not provide"
        ))
    }
}
