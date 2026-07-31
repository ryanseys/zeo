//! `Zlib::ZStream` -- the counters, lifecycle and buffer views that both
//! directions share, and that `Deflate`/`Inflate` inherit through the MRO.
//!
//! CRuby never constructs a `ZStream` directly (it has no `new`), and neither
//! does this: every row here reaches its state through [`codec::zs_of`], which
//! only ever sees a `Deflate` or an `Inflate`.

use super::codec::{self, Codec, Wrap, ready, zs_of};
use crate::RubyValue;
use zeo_macros::ruby_class;

ruby_class! {
    ZStream = zeo_abi::ZLIB_ZSTREAM_CLASS < zeo_abi::OBJECT_CLASS;

    // The counters. `total_in`/`total_out` include the container's framing,
    // as zlib's do.
    def "total_in" (recv) {
        Ok(RubyValue::Int(ready(recv)?.total_in as i64))
    }
    def "total_out" (recv) {
        Ok(RubyValue::Int(ready(recv)?.total_out as i64))
    }
    // The running Adler-32 (zlib framing) or CRC-32 (gzip framing) -- zlib
    // overloads one field for both, and reports whichever the container uses.
    def "adler" (recv) {
        Ok(RubyValue::Int(i64::from(ready(recv)?.adler)))
    }
    def "data_type" (recv) {
        Ok(RubyValue::Int(ready(recv)?.data_type))
    }

    // Lifecycle. `finished?`/`stream_end?` ask whether the DATA ended;
    // `closed?`/`ended?` whether the caller shut the stream down. CRuby
    // aliases each pair, and so does this.
    def "finished?" (recv) {
        Ok(RubyValue::Bool(ready(recv)?.finished))
    }
    def "stream_end?" (recv) {
        Ok(RubyValue::Bool(ready(recv)?.finished))
    }
    def "closed?" (recv) {
        Ok(RubyValue::Bool(zs_of(recv).state.lock().closed))
    }
    def "ended?" (recv) {
        Ok(RubyValue::Bool(zs_of(recv).state.lock().closed))
    }
    def "close" (recv) {
        zs_of(recv).state.lock().closed = true;
        Ok(RubyValue::Nil)
    }
    def "end" (recv) {
        zs_of(recv).state.lock().closed = true;
        Ok(RubyValue::Nil)
    }

    // Start over with the same settings, discarding everything queued.
    def "reset" (recv) {
        let st = &mut *ready(recv)?;
        match &mut st.codec {
            Codec::Deflate(d) => {
                d.comp.reset();
                d.header_written = false;
                d.crc = 0;
                d.size = 0;
                st.adler = if d.wrap == Wrap::Gzip { 0 } else { 1 };
            }
            Codec::Inflate(i) => {
                i.dec.reset(i.wrap == Wrap::Zlib);
                i.header_pending.clear();
                i.header_done = false;
                i.header = None;
                i.trailer.clear();
                i.crc = 0;
                i.size = 0;
                i.footer_checked = false;
                st.adler = if i.wrap == Wrap::Gzip { 0 } else { 1 };
            }
        }
        st.out.clear();
        st.total_in = 0;
        st.total_out = 0;
        st.finished = false;
        st.data_type = 2;
        Ok(RubyValue::Nil)
    }

    // Run to the end of the stream and hand back everything queued.
    def "finish" arity -1 (recv, _arg?) {
        codec::finish(recv)
    }

    // The two buffer views. Input is always consumed whole, so `avail_in` is 0
    // and `flush_next_in` is always empty; output is queued, so
    // `flush_next_out` drains it.
    def "avail_in" (recv) {
        drop(ready(recv)?);
        Ok(RubyValue::Int(0))
    }
    def "flush_next_in" (recv) {
        drop(ready(recv)?);
        Ok(super::bin_str(Vec::new()))
    }
    def "flush_next_out" (recv) {
        let st = &mut *ready(recv)?;
        Ok(super::bin_str(std::mem::take(&mut st.out)))
    }
    def "avail_out" (recv) {
        Ok(RubyValue::Int(ready(recv)?.avail_out))
    }
    // zlib sizes its output buffer with this. zeo grows its own on demand, so
    // the value is recorded and reported back but changes nothing.
    def "avail_out=" (recv, arg) {
        let n = crate::builtins::convert::to_index(arg)?;
        ready(recv)?.avail_out = n;
        Ok((*arg).clone())
    }
}
