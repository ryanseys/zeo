//! The edits zeo makes to upstream's headers, applied when the tree is
//! fetched.
//!
//! A zeo heap `VALUE` is a handle whose first two words are a real `struct
//! RBasic`. That much is MRI's layout, so `RB_FL_TEST_RAW`, `RB_BUILTIN_TYPE`,
//! `RB_TYPE_P`, `OBJ_FROZEN` and `RTYPEDDATA_P` need no edit, and an
//! extension that sets `FL_USER3` writes the word that owns it. Behind those
//! two words there is no `struct RString` and no `struct RArray`, so every
//! macro that reads object layout becomes a call. One rule covers them all:
//!
//! > A payload struct carries UPSTREAM'S LAYOUT. `X(obj)` is a call, not a
//! > cast, and answers a view the runtime owns. The view ALIASES real storage
//! > wherever zeo owns that storage as C-shaped memory, and is REFILLED from
//! > the object on every reach wherever it does not. A field zeo has no
//! > answer for is zero, and the accessor named for it raises.
//!
//! So every struct body stays upstream's, and the whole delta is a handful
//! of macro definitions plus one include line per file. Three groups:
//!
//! 1. **The eight cast macros** call the view entries `zeo.h` declares.
//!    `RData` and `RTypedData` alias the object's own cell, so
//!    `RTYPEDDATA(o)->data = p` writes the slot and cannot go stale (what
//!    date's `d_lite_marshal_load` needs); the other six refill. Three
//!    macros change beyond the cast: `RARRAY_ASET` calls in, because the
//!    pointer `RARRAY_PTR_USE` hands out is a projection and a store through
//!    it would stay there; `RMATCH_EXT` walks off the view rather than off
//!    the `VALUE`; `RREGEXP_PTR` raises, because the compiled pattern is
//!    zeo's engine's and may be recompiled.
//! 2. **The encoding index and coderange** are calls. Both read the flags
//!    word upstream, and a zeo String keeps its encoding in the object and
//!    derives its coderange from the bytes. Reading the bits does not merely
//!    answer "unknown": index 0 IS an encoding (ASCII-8BIT), and `fast_blank`
//!    decoded every UTF-8 string as binary until this. Filling the bits at
//!    pin time would be worse, since `force_encoding` leaves them stale.
//!    `RB_ENC_CODERANGE_SET` becomes a no-op: the value is derived, so there
//!    is nothing to assign.
//! 3. **`RBASIC_CLASS`** is a call. A handle's `klass` word is minted lazily
//!    and starts as `Qnil`; a direct read handed C nil where MRI hands the
//!    class. The call fills the word once and answers it.
//!
//! Every `old` must match exactly once, so an upstream change to any of
//! these lines fails the fetch by file and macro rather than a gem's build
//! much later. The C inside `new` is the one accepted residue of C in this
//! repository: it is what keeps the `.h` files out of it.

use std::path::Path;

/// One edit: in `file`, the text `old` -- present exactly once -- becomes
/// `new`.
pub struct Hunk {
    pub file: &'static str,
    pub old: &'static str,
    pub new: &'static str,
}

/// The include line every edited core header gains, placed after the
/// `value_type.h` include (or the last one, where there is none).
const ZEO_INCLUDE: &str = "#include \"ruby/internal/zeo.h\"\n";

macro_rules! include_after {
    ($file:literal, $line:literal) => {
        Hunk {
            file: $file,
            old: concat!($line, "\n"),
            new: concat!($line, "\n", "#include \"ruby/internal/zeo.h\"\n"),
        }
    };
}

pub const HUNKS: &[Hunk] = &[
    // -- 1. the cast macros ------------------------------------------------
    include_after!("ruby/internal/core/rstring.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/rstring.h",
        old: "#define RSTRING(obj)            RBIMPL_CAST((struct RString *)(obj))\n",
        new: "/* zeo: a call, not a cast -- a refilled view. zeo always answers the\n\
              \x20* non-embedded shape and sets ::RSTRING_NOEMBED on the object, so the arms\n\
              \x20* below take `as.heap`. The BYTES `as.heap.ptr` names are the String's own,\n\
              \x20* so a write through them reaches it; a write to `len` changes only the\n\
              \x20* view. See `ruby/internal/zeo.h`. */\n\
              #define RSTRING(obj)            rb_zeo_rstring(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    include_after!("ruby/internal/core/rarray.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/rarray.h",
        old: "#define RARRAY(obj)            RBIMPL_CAST((struct RArray *)(obj))\n",
        new: "/* zeo: a call, not a cast -- a refilled view. A zeo Array element is a Rust\n\
              \x20* enum rather than a tagged word, so `as.heap.ptr` names a PROJECTION built\n\
              \x20* beside the object. zeo never sets ::RARRAY_EMBED_FLAG, so the arms below\n\
              \x20* take `as.heap`. See `ruby/internal/zeo.h`. */\n\
              #define RARRAY(obj)            rb_zeo_rarray(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    Hunk {
        file: "ruby/internal/core/rarray.h",
        old: "    RARRAY_PTR_USE(ary, ptr,\n        RB_OBJ_WRITE(ary, &ptr[i], v));\n",
        new: "    /* zeo: the pointer `RARRAY_PTR_USE` hands out is a projection built\n\
              \x20    * beside the object, so upstream's store through it would stay in the\n\
              \x20    * projection. This one reaches the Array. */\n\
              \x20   rb_zeo_ary_aset(ary, i, v);\n",
    },
    include_after!("ruby/internal/core/robject.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/robject.h",
        old: "#define ROBJECT(obj)          RBIMPL_CAST((struct RObject *)(obj))\n",
        new: "/* zeo: a call, not a cast -- a refilled view. zeo has no `VALUE` ivar array\n\
              \x20* to point at, so `as.heap.fields` names one materialized beside the object,\n\
              \x20* and zeo sets ::ROBJECT_HEAP so the arm below takes it. A store through it\n\
              \x20* does not reach the object. See `ruby/internal/zeo.h`. */\n\
              #define ROBJECT(obj)          rb_zeo_robject(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    include_after!("ruby/internal/core/rregexp.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/rregexp.h",
        old: "#define RREGEXP(obj)     RBIMPL_CAST((struct RRegexp *)(obj))\n",
        new: "/* zeo: a call, not a cast -- a refilled view. `src` and `usecnt` are real;\n\
              \x20* `ptr` is zero, and ::RREGEXP_PTR raises rather than hand it out. See\n\
              \x20* `ruby/internal/zeo.h`. */\n\
              #define RREGEXP(obj)     rb_zeo_rregexp(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    Hunk {
        file: "ruby/internal/core/rregexp.h",
        old: "#define RREGEXP_PTR(obj) (RREGEXP(obj)->ptr)\n",
        new: "/* zeo: raises. The compiled pattern belongs to zeo's own regexp engine,\n\
              \x20* which may recompile it, so handing the pointer out would let an extension\n\
              \x20* call onig against a buffer zeo owns. `rb_reg_prepare_re` -- MRI's\n\
              \x20* supported way to get one -- refuses for the same reason. */\n\
              #define RREGEXP_PTR(obj) \\\n\
              \x20   RBIMPL_CAST((struct re_pattern_buffer *)rb_zeo_no_field(\"RREGEXP_PTR\"))\n",
    },
    include_after!("ruby/internal/core/rmatch.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/rmatch.h",
        old: "#define RMATCH(obj) RBIMPL_CAST((struct RMatch *)(obj))\n",
        new: "/* zeo: a call, not a cast -- a refilled view. The block it answers carries a\n\
              \x20* `struct RMatch` followed by the `rb_matchext_t` ::RMATCH_EXT reaches by\n\
              \x20* pointer arithmetic, and the registers come from the MatchData's own group\n\
              \x20* offsets. See `ruby/internal/zeo.h`. */\n\
              #define RMATCH(obj) rb_zeo_rmatch(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    Hunk {
        file: "ruby/internal/core/rmatch.h",
        old: "#define RMATCH_EXT(m) ((rb_matchext_t *)((char *)(m) + sizeof(struct RMatch)))\n",
        new: "/* zeo: from the view rather than from the VALUE. Upstream's `m` IS the\n\
              \x20* `struct RMatch`; zeo's is a handle, and the block ::RMATCH answers carries\n\
              \x20* the `rb_matchext_t` right behind the struct, which is what this walks\n\
              \x20* onto. */\n\
              #define RMATCH_EXT(m) ((rb_matchext_t *)((char *)RMATCH(m) + sizeof(struct RMatch)))\n",
    },
    include_after!("ruby/internal/core/rfile.h", "#include \"ruby/internal/cast.h\""),
    Hunk {
        file: "ruby/internal/core/rfile.h",
        old: "#define RFILE(obj) RBIMPL_CAST((struct RFile *)(obj))\n",
        new: "/* zeo: a call, not a cast -- a refilled view. `fptr` names a `struct rb_io`\n\
              \x20* minted beside the object and filled from the IO on every reach, so\n\
              \x20* `GetOpenFile(io, fp); fp->fd` answers the descriptor the IO has right now.\n\
              \x20* The fields zeo cannot honour -- the buffers, the converters -- stay zero,\n\
              \x20* and a store to the view does not reach the IO. See\n\
              \x20* `ruby/internal/zeo.h`. */\n\
              #define RFILE(obj) rb_zeo_rfile(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    include_after!("ruby/internal/core/rdata.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/rdata.h",
        old: "#define RDATA(obj)                RBIMPL_CAST((struct RData *)(obj))\n",
        new: "/* zeo: a call, not a cast. The cell it answers lives inside the object, so\n\
              \x20* `RDATA(o)->data = p` writes the object's own slot. See\n\
              \x20* `ruby/internal/zeo.h`. */\n\
              #define RDATA(obj)                rb_zeo_rdata(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    include_after!("ruby/internal/core/rtypeddata.h", "#include \"ruby/internal/value_type.h\""),
    Hunk {
        file: "ruby/internal/core/rtypeddata.h",
        old: "#define RTYPEDDATA(obj)              RBIMPL_CAST((struct RTypedData *)(obj))\n",
        new: "/* zeo: a call, not a cast. The cell it answers lives inside the object, so\n\
              \x20* `RTYPEDDATA(o)->data = p` writes the object's own slot and cannot go\n\
              \x20* stale. See `ruby/internal/zeo.h`. */\n\
              #define RTYPEDDATA(obj)              rb_zeo_rtypeddata(RBIMPL_CAST((VALUE)(obj)))\n",
    },
    // -- 2. the encoding index and coderange -------------------------------
    Hunk {
        file: "ruby/internal/encoding/coderange.h",
        old: "static inline enum ruby_coderange_type\n\
              RB_ENC_CODERANGE(VALUE obj)\n\
              {\n\
              \x20   VALUE ret = RB_FL_TEST_RAW(obj, RUBY_ENC_CODERANGE_MASK);\n\
              \n\
              \x20   return RBIMPL_CAST((enum ruby_coderange_type)ret);\n\
              }\n",
        new: "/* zeo: declared here because this header is included before string.h,\n\
              \x20* which is where upstream declares it. */\n\
              int rb_enc_str_coderange(VALUE str);\n\
              \n\
              static inline enum ruby_coderange_type\n\
              RB_ENC_CODERANGE(VALUE obj)\n\
              {\n\
              \x20   /* zeo: the coderange is computed from the bytes and cached inside the\n\
              \x20    * String, not stored in the flags word. Reading the bits would answer\n\
              \x20    * UNKNOWN for every string, and a caller that trusts UNKNOWN rescans --\n\
              \x20    * which is correct but wasteful -- while one that trusts a stale 7BIT\n\
              \x20    * is wrong. The call answers what the string actually is. */\n\
              \x20   return RBIMPL_CAST((enum ruby_coderange_type)rb_enc_str_coderange(obj));\n\
              }\n",
    },
    Hunk {
        file: "ruby/internal/encoding/coderange.h",
        old: "static inline void\n\
              RB_ENC_CODERANGE_SET(VALUE obj, enum ruby_coderange_type cr)\n\
              {\n\
              \x20   RB_FL_UNSET_RAW(obj, RUBY_ENC_CODERANGE_MASK);\n\
              \x20   RB_FL_SET_RAW(obj, cr);\n\
              }\n",
        new: "static inline void\n\
              RB_ENC_CODERANGE_SET(VALUE obj, enum ruby_coderange_type cr)\n\
              {\n\
              \x20   /* zeo: the coderange is derived from the bytes, so there is nothing to\n\
              \x20    * assign. A caller sets it to record what it already knows; zeo\n\
              \x20    * recomputes on demand and reaches the same answer. */\n\
              \x20   (void)obj;\n\
              \x20   (void)cr;\n\
              }\n",
    },
    Hunk {
        file: "ruby/internal/encoding/encoding.h",
        old: "static inline int\n\
              RB_ENCODING_GET_INLINED(VALUE obj)\n\
              {\n\
              \x20   VALUE ret = RB_FL_TEST_RAW(obj, RUBY_ENCODING_MASK) >> RUBY_ENCODING_SHIFT;\n\
              \n\
              \x20   return RBIMPL_CAST((int)ret);\n\
              }\n",
        new: "/* zeo: declared here because this inline is defined above upstream's own\n\
              \x20* declaration, which now has a caller. */\n\
              int rb_enc_get_index(VALUE obj);\n\
              \n\
              static inline int\n\
              RB_ENCODING_GET_INLINED(VALUE obj)\n\
              {\n\
              \x20   /* zeo: a zeo String keeps its encoding in the object, not in the flags\n\
              \x20    * word, and `force_encoding` moves it -- so the bits here would be a\n\
              \x20    * snapshot that goes stale rather than merely absent. The call always\n\
              \x20    * answers what the string carries NOW. */\n\
              \x20   return rb_enc_get_index(obj);\n\
              }\n",
    },
    Hunk {
        file: "ruby/internal/encoding/encoding.h",
        old: "static inline int\n\
              RB_ENCODING_GET(VALUE obj)\n\
              {\n\
              \x20   int encindex = RB_ENCODING_GET_INLINED(obj);\n\
              \n\
              \x20   if (encindex == RUBY_ENCODING_INLINE_MAX) {\n\
              \x20       return rb_enc_get_index(obj);\n\
              \x20   }\n\
              \x20   else {\n\
              \x20       return encindex;\n\
              \x20   }\n\
              }\n",
        new: "static inline int\n\
              RB_ENCODING_GET(VALUE obj)\n\
              {\n\
              \x20   /* zeo: see RB_ENCODING_GET_INLINED. There is no inline half to try. */\n\
              \x20   return rb_enc_get_index(obj);\n\
              }\n",
    },
    // -- 3. RBASIC_CLASS ---------------------------------------------------
    include_after!("ruby/internal/core/rbasic.h", "#include \"ruby/internal/value.h\""),
    Hunk {
        file: "ruby/internal/core/rbasic.h",
        old: "    RBIMPL_ASSERT_OR_ASSUME(! RB_SPECIAL_CONST_P(obj));\n\
              \x20   return RBASIC(obj)->klass;\n",
        new: "    RBIMPL_ASSERT_OR_ASSUME(! RB_SPECIAL_CONST_P(obj));\n\
              \x20   /* zeo: a call, not a field read. A handle's `klass` word is minted\n\
              \x20    * lazily -- it starts as Qnil, and a direct read handed C nil where MRI\n\
              \x20    * hands the class (a Class's metaclass included, which is what\n\
              \x20    * `rb_undef_method(CLASS_OF(c), \"m\")` names). The call fills the word\n\
              \x20    * once and answers it. See `ruby/internal/zeo.h`. */\n\
              \x20   return rb_zeo_rbasic_class(obj);\n",
    },
];

/// Apply every hunk to the tree under `include`, each exactly once.
pub fn apply_all(include: &Path) -> Result<(), String> {
    for hunk in HUNKS {
        let path = include.join(hunk.file);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e} (is this upstream's include/ tree?)", path.display()))?;
        let edited = apply(&text, hunk)?;
        std::fs::write(&path, edited).map_err(|e| format!("writing {}: {e}", path.display()))?;
    }
    Ok(())
}

/// `text` with the hunk applied, or why it does not apply.
pub fn apply(text: &str, hunk: &Hunk) -> Result<String, String> {
    match text.matches(hunk.old).count() {
        1 => Ok(text.replacen(hunk.old, hunk.new, 1)),
        0 => Err(format!(
            "{}: the text zeo edits is not there -- upstream changed it, and the hunk that \
             starts {:?} needs a review",
            hunk.file,
            hunk.old.lines().next().unwrap_or_default()
        )),
        n => Err(format!(
            "{}: the text zeo edits appears {n} times, so the hunk that starts {:?} is ambiguous",
            hunk.file,
            hunk.old.lines().next().unwrap_or_default()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule the hunks exist to enforce, in its two halves: every payload
    /// struct keeps upstream's body (nothing here touches a `struct X {`),
    /// and its cast macro reaches the view through the entry named for it.
    #[test]
    fn every_payload_cast_reaches_its_view_through_a_call() {
        for (name, entry) in [
            ("RString", "rstring"),
            ("RArray", "rarray"),
            ("RObject", "robject"),
            ("RRegexp", "rregexp"),
            ("RMatch", "rmatch"),
            ("RFile", "rfile"),
            ("RData", "rdata"),
            ("RTypedData", "rtypeddata"),
        ] {
            let file = format!("ruby/internal/core/{}.h", name.to_lowercase());
            let cast = HUNKS
                .iter()
                .find(|h| h.file == file && h.old.contains(&format!("(struct {name} *)(obj)")))
                .unwrap_or_else(|| panic!("no hunk turns {name}(obj) into a call"));
            assert!(
                cast.new.contains(&format!("rb_zeo_{entry}(RBIMPL_CAST((VALUE)(obj)))")),
                "{file} does not reach its view through rb_zeo_{entry}"
            );
            assert!(
                HUNKS.iter().any(|h| h.file == file && h.new.contains(ZEO_INCLUDE)),
                "{file} does not include zeo.h"
            );
        }
        assert!(
            HUNKS.iter().all(|h| !h.old.contains("struct ") || !h.old.contains(" {")),
            "a hunk edits a struct body"
        );
    }

    /// Every entry a hunk calls is one `zeo.h` declares.
    #[test]
    fn every_entry_a_hunk_calls_is_declared() {
        let declared = super::super::zeo_h::render();
        for hunk in HUNKS {
            for word in hunk.new.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if word.starts_with("rb_zeo_") {
                    assert!(declared.contains(&format!(" {word}(")), "{word} is not in zeo.h");
                }
            }
        }
    }

    #[test]
    fn a_hunk_applies_exactly_once() {
        let hunk = Hunk {
            file: "x.h",
            old: "a\n",
            new: "b\n",
        };
        assert_eq!(apply("a\nc\n", &hunk).unwrap(), "b\nc\n");
        assert!(apply("c\n", &hunk).unwrap_err().contains("not there"));
        assert!(apply("a\na\n", &hunk).unwrap_err().contains("2 times"));
    }
}
