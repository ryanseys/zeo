//! MRI's object layout, written by hand and held to what a C compiler
//! measured.
//!
//! Every struct here is one an extension reads through a view, and the Rust
//! that fills a view has to agree with the header to the byte -- a wrong
//! offset reads or writes an unrelated field, silently. So the layout is not
//! trusted: [`super::layout_facts`] holds the sizes, offsets and constants
//! `cargo xtask check-c-headers layout` measured from the pinned headers with a C
//! compiler, and the test below holds every field here to that record. A
//! header bump that moves a field fails `cext layout --check` first and this
//! crate's tests second, both by field name.
//!
//! Names keep C's spelling so a fill site reads like the header it mirrors.
//! Nothing here is public API: [`super::view`] fills these and
//! [`super::handles`] writes the flag constants into an object.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};

pub type VALUE = c_ulong;
pub type RUBY_DATA_FUNC = Option<unsafe extern "C" fn(*mut c_void)>;
pub type OnigPosition = isize;
pub type rb_pid_t = c_int;
pub type rb_io_mode = c_uint;
pub type rb_data_type_t = rb_data_type_struct;
pub type rb_io_buffer_t = rb_io_internal_buffer;

/// A type an extension only ever holds a pointer to.
macro_rules! opaque {
    ($($name:ident),* $(,)?) => {$(
        #[repr(C)]
        pub struct $name {
            _opaque: [u8; 0],
        }
    )*};
}
opaque!(FILE, rb_econv_t, re_pattern_buffer, OnigEncodingTypeST);

/// The first two words of every heap object, and of every zeo handle.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RBasic {
    pub flags: VALUE,
    pub klass: VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RString {
    pub basic: RBasic,
    /// Bytes, not characters, and not counting the terminating NUL.
    pub len: c_long,
    pub as_: RStringPayload,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RStringPayload {
    pub heap: RStringHeap,
    pub embed: RStringEmbed,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RStringHeap {
    pub ptr: *mut c_char,
    pub aux: RStringAux,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RStringAux {
    pub capa: c_long,
    pub shared: VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RStringEmbed {
    pub ary: [c_char; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RArray {
    pub basic: RBasic,
    pub as_: RArrayPayload,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RArrayPayload {
    pub heap: RArrayHeap,
    pub ary: [VALUE; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RArrayHeap {
    pub len: c_long,
    pub aux: RArrayAux,
    pub ptr: *const VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RArrayAux {
    pub capa: c_long,
    pub shared_root: VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RObject {
    pub basic: RBasic,
    pub as_: RObjectPayload,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RObjectPayload {
    pub heap: RObjectHeap,
    pub ary: [VALUE; 1],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RObjectHeap {
    pub fields: *mut VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RRegexp {
    pub basic: RBasic,
    pub ptr: *mut re_pattern_buffer,
    pub src: VALUE,
    pub usecnt: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RMatch {
    pub basic: RBasic,
    pub str_: VALUE,
    pub regexp: VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct re_registers {
    pub allocated: c_int,
    pub num_regs: c_int,
    pub beg: *mut OnigPosition,
    pub end: *mut OnigPosition,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct rmatch_offset {
    pub beg: c_long,
    pub end: c_long,
}

/// `rb_matchext_t`, the block `RMATCH_EXT` reaches right behind `RMatch`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct rb_matchext_struct {
    pub regs: re_registers,
    pub char_offset: *mut rmatch_offset,
    pub char_offset_num_allocated: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RFile {
    pub basic: RBasic,
    pub fptr: *mut rb_io,
}

/// Packed, as `ruby/io.h` declares it (`RBIMPL_ATTR_PACKED_STRUCT_UNALIGNED`),
/// which is what puts the fields after it in `rb_io` where C puts them.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct rb_io_internal_buffer {
    pub ptr: *mut c_char,
    pub off: c_int,
    pub len: c_int,
    pub capa: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct rb_io_encoding {
    pub enc: *const OnigEncodingTypeST,
    pub enc2: *const OnigEncodingTypeST,
    pub ecflags: c_int,
    pub ecopts: VALUE,
}

/// `rb_io_t`: MRI's own IO struct, which an extension may hold for as long
/// as the IO lives.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct rb_io {
    pub self_: VALUE,
    pub stdio_file: *mut FILE,
    pub fd: c_int,
    pub mode: rb_io_mode,
    pub pid: rb_pid_t,
    pub lineno: c_int,
    pub pathv: VALUE,
    pub finalize: Option<unsafe extern "C" fn(*mut rb_io, c_int)>,
    pub wbuf: rb_io_buffer_t,
    pub rbuf: rb_io_buffer_t,
    pub tied_io_for_writing: VALUE,
    pub encs: rb_io_encoding,
    pub readconv: *mut rb_econv_t,
    pub cbuf: rb_io_buffer_t,
    pub writeconv: *mut rb_econv_t,
    pub writeconv_asciicompat: VALUE,
    pub writeconv_initialized: c_int,
    pub writeconv_pre_ecflags: c_int,
    pub writeconv_pre_ecopts: VALUE,
    pub write_lock: VALUE,
    pub timeout: VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RData {
    pub basic: RBasic,
    pub dmark: RUBY_DATA_FUNC,
    pub dfree: RUBY_DATA_FUNC,
    /// After `dmark` and `dfree` so that `DATA_PTR` reads the same offset
    /// for `RData` and a non-embedded `RTypedData`.
    pub data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RTypedData {
    pub basic: RBasic,
    pub fields_obj: VALUE,
    /// `const rb_data_type_t *` with the low bit set for an embedded data.
    pub type_: VALUE,
    pub data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct rb_data_type_struct {
    pub wrap_struct_name: *const c_char,
    pub function: RTypedDataFns,
    pub parent: *const rb_data_type_t,
    pub data: *mut c_void,
    pub flags: VALUE,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RTypedDataFns {
    pub dmark: RUBY_DATA_FUNC,
    pub dfree: RUBY_DATA_FUNC,
    pub dsize: Option<unsafe extern "C" fn(*const c_void) -> usize>,
    pub dcompact: RUBY_DATA_FUNC,
    pub reserved: [*mut c_void; 1],
}

/// The three shape flags zeo sets on an object so that upstream's own
/// accessors take the arm a view can answer, plus the flags the handle
/// writes and the two the encoding index rides in.
pub const RSTRING_NOEMBED: usize = 1 << 13;
pub const ROBJECT_HEAP: usize = 1 << 16;
pub const RARRAY_EMBED_FLAG: usize = 1 << 13;
pub const FL_FREEZE: usize = 1 << 11;
pub const FL_IS_TYPED_DATA: usize = 1 << 6;
pub const FL_USER8: usize = 1 << 20;
pub const FL_USER9: usize = 1 << 21;

#[cfg(test)]
mod tests {
    use super::super::layout_facts as facts;
    use super::super::st::StTable;
    use super::*;
    use std::collections::BTreeSet;
    use std::mem::{MaybeUninit, offset_of, size_of};

    fn size_of_pointee<T>(_: *const T) -> usize {
        size_of::<T>()
    }

    /// A struct's size, or a field's offset and size, checked against the
    /// measured record and ticked off so nothing measured goes unchecked.
    struct Checked {
        seen: BTreeSet<(&'static str, &'static str)>,
    }

    impl Checked {
        fn size(&mut self, c: &'static str, have: usize) {
            let (_, want) = facts::SIZES
                .iter()
                .find(|(name, _)| *name == c)
                .unwrap_or_else(|| panic!("{c} was not measured; run `cargo xtask check-c-headers layout`"));
            assert_eq!(have, *want, "sizeof({c})");
            self.seen.insert((c, ""));
        }

        fn field(&mut self, c: &'static str, f: &'static str, offset: usize, size: usize) {
            let &(_, _, want_offset, want_size) = facts::FIELDS
                .iter()
                .find(|(name, field, _, _)| *name == c && *field == f)
                .unwrap_or_else(|| {
                    panic!("{c}.{f} was not measured; run `cargo xtask check-c-headers layout`")
                });
            assert_eq!(offset, want_offset, "offsetof({c}, {f})");
            assert_eq!(size, want_size, "sizeof({c}.{f})");
            self.seen.insert((c, f));
        }
    }

    macro_rules! field {
        ($ck:ident, $c:literal, $f:literal, $T:ty, $($p:tt)+) => {{
            let slot = MaybeUninit::<$T>::uninit();
            // SAFETY: `&raw const` names the place without reading it.
            let at = unsafe { &raw const (*slot.as_ptr()).$($p)+ };
            $ck.field($c, $f, offset_of!($T, $($p)+), size_of_pointee(at));
        }};
    }

    /// Every row the C compiler measured has a Rust field at the same place
    /// and of the same width, and every Rust field here is measured.
    #[test]
    fn every_struct_and_field_matches_the_measured_headers() {
        let mut ck = Checked {
            seen: BTreeSet::new(),
        };
        ck.size("struct RBasic", size_of::<RBasic>());
        field!(ck, "struct RBasic", "flags", RBasic, flags);
        field!(ck, "struct RBasic", "klass", RBasic, klass);

        ck.size("struct RString", size_of::<RString>());
        field!(ck, "struct RString", "basic", RString, basic);
        field!(ck, "struct RString", "len", RString, len);
        field!(ck, "struct RString", "as.heap.ptr", RString, as_.heap.ptr);
        field!(
            ck,
            "struct RString",
            "as.heap.aux.capa",
            RString,
            as_.heap.aux.capa
        );
        field!(
            ck,
            "struct RString",
            "as.heap.aux.shared",
            RString,
            as_.heap.aux.shared
        );
        field!(ck, "struct RString", "as.embed.ary", RString, as_.embed.ary);

        ck.size("struct RArray", size_of::<RArray>());
        field!(ck, "struct RArray", "basic", RArray, basic);
        field!(ck, "struct RArray", "as.heap.len", RArray, as_.heap.len);
        field!(
            ck,
            "struct RArray",
            "as.heap.aux.capa",
            RArray,
            as_.heap.aux.capa
        );
        field!(
            ck,
            "struct RArray",
            "as.heap.aux.shared_root",
            RArray,
            as_.heap.aux.shared_root
        );
        field!(ck, "struct RArray", "as.heap.ptr", RArray, as_.heap.ptr);
        field!(ck, "struct RArray", "as.ary", RArray, as_.ary);

        ck.size("struct RObject", size_of::<RObject>());
        field!(ck, "struct RObject", "basic", RObject, basic);
        field!(
            ck,
            "struct RObject",
            "as.heap.fields",
            RObject,
            as_.heap.fields
        );
        field!(ck, "struct RObject", "as.ary", RObject, as_.ary);

        ck.size("struct RRegexp", size_of::<RRegexp>());
        field!(ck, "struct RRegexp", "basic", RRegexp, basic);
        field!(ck, "struct RRegexp", "ptr", RRegexp, ptr);
        field!(ck, "struct RRegexp", "src", RRegexp, src);
        field!(ck, "struct RRegexp", "usecnt", RRegexp, usecnt);

        ck.size("struct RMatch", size_of::<RMatch>());
        field!(ck, "struct RMatch", "basic", RMatch, basic);
        field!(ck, "struct RMatch", "str", RMatch, str_);
        field!(ck, "struct RMatch", "regexp", RMatch, regexp);

        ck.size("rb_matchext_t", size_of::<rb_matchext_struct>());
        field!(
            ck,
            "rb_matchext_t",
            "regs.allocated",
            rb_matchext_struct,
            regs.allocated
        );
        field!(
            ck,
            "rb_matchext_t",
            "regs.num_regs",
            rb_matchext_struct,
            regs.num_regs
        );
        field!(
            ck,
            "rb_matchext_t",
            "regs.beg",
            rb_matchext_struct,
            regs.beg
        );
        field!(
            ck,
            "rb_matchext_t",
            "regs.end",
            rb_matchext_struct,
            regs.end
        );
        field!(
            ck,
            "rb_matchext_t",
            "char_offset",
            rb_matchext_struct,
            char_offset
        );
        field!(
            ck,
            "rb_matchext_t",
            "char_offset_num_allocated",
            rb_matchext_struct,
            char_offset_num_allocated
        );

        ck.size("struct rmatch_offset", size_of::<rmatch_offset>());
        field!(ck, "struct rmatch_offset", "beg", rmatch_offset, beg);
        field!(ck, "struct rmatch_offset", "end", rmatch_offset, end);

        ck.size("struct RFile", size_of::<RFile>());
        field!(ck, "struct RFile", "basic", RFile, basic);
        field!(ck, "struct RFile", "fptr", RFile, fptr);

        ck.size(
            "struct rb_io_internal_buffer",
            size_of::<rb_io_internal_buffer>(),
        );
        field!(
            ck,
            "struct rb_io_internal_buffer",
            "ptr",
            rb_io_internal_buffer,
            ptr
        );
        field!(
            ck,
            "struct rb_io_internal_buffer",
            "off",
            rb_io_internal_buffer,
            off
        );
        field!(
            ck,
            "struct rb_io_internal_buffer",
            "len",
            rb_io_internal_buffer,
            len
        );
        field!(
            ck,
            "struct rb_io_internal_buffer",
            "capa",
            rb_io_internal_buffer,
            capa
        );

        ck.size("struct rb_io_encoding", size_of::<rb_io_encoding>());
        field!(ck, "struct rb_io_encoding", "enc", rb_io_encoding, enc);
        field!(ck, "struct rb_io_encoding", "enc2", rb_io_encoding, enc2);
        field!(
            ck,
            "struct rb_io_encoding",
            "ecflags",
            rb_io_encoding,
            ecflags
        );
        field!(
            ck,
            "struct rb_io_encoding",
            "ecopts",
            rb_io_encoding,
            ecopts
        );

        ck.size("struct rb_io", size_of::<rb_io>());
        field!(ck, "struct rb_io", "self", rb_io, self_);
        field!(ck, "struct rb_io", "stdio_file", rb_io, stdio_file);
        field!(ck, "struct rb_io", "fd", rb_io, fd);
        field!(ck, "struct rb_io", "mode", rb_io, mode);
        field!(ck, "struct rb_io", "pid", rb_io, pid);
        field!(ck, "struct rb_io", "lineno", rb_io, lineno);
        field!(ck, "struct rb_io", "pathv", rb_io, pathv);
        field!(ck, "struct rb_io", "finalize", rb_io, finalize);
        field!(ck, "struct rb_io", "wbuf", rb_io, wbuf);
        field!(ck, "struct rb_io", "rbuf", rb_io, rbuf);
        field!(
            ck,
            "struct rb_io",
            "tied_io_for_writing",
            rb_io,
            tied_io_for_writing
        );
        field!(ck, "struct rb_io", "encs", rb_io, encs);
        field!(ck, "struct rb_io", "readconv", rb_io, readconv);
        field!(ck, "struct rb_io", "cbuf", rb_io, cbuf);
        field!(ck, "struct rb_io", "writeconv", rb_io, writeconv);
        field!(
            ck,
            "struct rb_io",
            "writeconv_asciicompat",
            rb_io,
            writeconv_asciicompat
        );
        field!(
            ck,
            "struct rb_io",
            "writeconv_initialized",
            rb_io,
            writeconv_initialized
        );
        field!(
            ck,
            "struct rb_io",
            "writeconv_pre_ecflags",
            rb_io,
            writeconv_pre_ecflags
        );
        field!(
            ck,
            "struct rb_io",
            "writeconv_pre_ecopts",
            rb_io,
            writeconv_pre_ecopts
        );
        field!(ck, "struct rb_io", "write_lock", rb_io, write_lock);
        field!(ck, "struct rb_io", "timeout", rb_io, timeout);

        ck.size("struct RData", size_of::<RData>());
        field!(ck, "struct RData", "basic", RData, basic);
        field!(ck, "struct RData", "dmark", RData, dmark);
        field!(ck, "struct RData", "dfree", RData, dfree);
        field!(ck, "struct RData", "data", RData, data);

        ck.size("struct RTypedData", size_of::<RTypedData>());
        field!(ck, "struct RTypedData", "basic", RTypedData, basic);
        field!(
            ck,
            "struct RTypedData",
            "fields_obj",
            RTypedData,
            fields_obj
        );
        field!(ck, "struct RTypedData", "type", RTypedData, type_);
        field!(ck, "struct RTypedData", "data", RTypedData, data);

        ck.size("rb_data_type_t", size_of::<rb_data_type_t>());
        field!(
            ck,
            "rb_data_type_t",
            "wrap_struct_name",
            rb_data_type_t,
            wrap_struct_name
        );
        field!(
            ck,
            "rb_data_type_t",
            "function.dmark",
            rb_data_type_t,
            function.dmark
        );
        field!(
            ck,
            "rb_data_type_t",
            "function.dfree",
            rb_data_type_t,
            function.dfree
        );
        field!(
            ck,
            "rb_data_type_t",
            "function.dsize",
            rb_data_type_t,
            function.dsize
        );
        field!(
            ck,
            "rb_data_type_t",
            "function.dcompact",
            rb_data_type_t,
            function.dcompact
        );
        field!(
            ck,
            "rb_data_type_t",
            "function.reserved",
            rb_data_type_t,
            function.reserved
        );
        field!(ck, "rb_data_type_t", "parent", rb_data_type_t, parent);
        field!(ck, "rb_data_type_t", "data", rb_data_type_t, data);
        field!(ck, "rb_data_type_t", "flags", rb_data_type_t, flags);

        ck.size("struct st_table", size_of::<StTable>());
        field!(ck, "struct st_table", "entry_power", StTable, entry_power);
        field!(ck, "struct st_table", "bin_power", StTable, bin_power);
        field!(ck, "struct st_table", "size_ind", StTable, size_ind);
        field!(ck, "struct st_table", "rebuilds_num", StTable, rebuilds_num);
        field!(ck, "struct st_table", "type", StTable, hash_type);
        field!(ck, "struct st_table", "num_entries", StTable, num_entries);
        field!(ck, "struct st_table", "bins", StTable, bins);
        field!(
            ck,
            "struct st_table",
            "entries_start",
            StTable,
            entries_start
        );
        field!(
            ck,
            "struct st_table",
            "entries_bound",
            StTable,
            entries_bound
        );
        field!(ck, "struct st_table", "entries", StTable, entries);

        let measured: BTreeSet<(&str, &str)> = facts::SIZES
            .iter()
            .map(|(c, _)| (*c, ""))
            .chain(facts::FIELDS.iter().map(|(c, f, _, _)| (*c, *f)))
            .collect();
        let unchecked: Vec<_> = measured.difference(&ck.seen).collect();
        assert!(
            unchecked.is_empty(),
            "measured but not held to a Rust field: {unchecked:?}"
        );
    }

    /// The flag constants are the header's.
    #[test]
    fn every_flag_constant_matches_the_measured_headers() {
        for (name, have) in [
            ("RSTRING_NOEMBED", RSTRING_NOEMBED),
            ("ROBJECT_HEAP", ROBJECT_HEAP),
            ("RARRAY_EMBED_FLAG", RARRAY_EMBED_FLAG),
            ("RUBY_FL_FREEZE", FL_FREEZE),
            ("RUBY_TYPED_FL_IS_TYPED_DATA", FL_IS_TYPED_DATA),
            ("RUBY_FL_USER8", FL_USER8),
            ("RUBY_FL_USER9", FL_USER9),
        ] {
            assert_eq!(have as u64, facts::measured(name), "{name}");
        }
    }
}
