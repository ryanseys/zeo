//! `ruby/internal/zeo.h`: the one header zeo adds to upstream's tree. It
//! declares the entries every edited cast macro calls and states the rule
//! once, so it is the one file a reader of an edited header has to find.
//!
//! Rendered from [`VIEW_ENTRIES`], which the crate's test holds to the
//! `rb_zeo_*` definitions in `view.rs`, `handles.rs` and `io.rs`: the
//! surface has one owner.

/// `(return type, name, parameters)` for every entry, in the order the
/// header declares them.
pub const VIEW_ENTRIES: &[(&str, &str, &str)] = &[
    ("VALUE", "rb_zeo_rbasic_class", "VALUE obj"),
    ("struct RString *", "rb_zeo_rstring", "VALUE obj"),
    ("struct RArray *", "rb_zeo_rarray", "VALUE obj"),
    ("struct RObject *", "rb_zeo_robject", "VALUE obj"),
    ("struct RRegexp *", "rb_zeo_rregexp", "VALUE obj"),
    ("struct RMatch *", "rb_zeo_rmatch", "VALUE obj"),
    ("struct RFile *", "rb_zeo_rfile", "VALUE obj"),
    ("struct RData *", "rb_zeo_rdata", "VALUE obj"),
    ("struct RTypedData *", "rb_zeo_rtypeddata", "VALUE obj"),
    ("void", "rb_zeo_ary_aset", "VALUE ary, long i, VALUE v"),
    ("void *", "rb_zeo_no_field", "const char *what"),
];

/// The header's text.
pub fn render() -> String {
    let mut h = String::from(
        "#ifndef RBIMPL_ZEO_H                                 /*-*-C++-*-vi:se ft=cpp:*/\n\
         #define RBIMPL_ZEO_H\n\
         /**\n\
         \x20* @file\n\
         \x20* @copyright  Added by zeo. Not part of upstream Ruby.\n\
         \x20* @brief      The view entries every payload-struct cast macro calls.\n\
         \x20*\n\
         \x20* A zeo heap `VALUE` is a handle whose first two words are a real\n\
         \x20* `struct RBasic`. So `RB_FL_TEST_RAW`, `RB_BUILTIN_TYPE`, `RB_TYPE_P`,\n\
         \x20* `RB_OBJ_FROZEN_RAW` and `RTYPEDDATA_P` read the truth with no edit at\n\
         \x20* all. The `klass` half is the one exception: it is minted lazily, so\n\
         \x20* `RBASIC_CLASS` is a call (below) rather than the field read.\n\
         \x20*\n\
         \x20* Behind those two words there is no `struct RString` and no `struct RArray`:\n\
         \x20* a zeo String is an `Arc<Mutex<StrBuf>>` and a zeo Array is a `Vec` the\n\
         \x20* runtime owns. So a cast to a payload struct would read bytes that mean\n\
         \x20* nothing, and every cast macro calls in here instead.\n\
         \x20*\n\
         \x20* One rule governs all of them:\n\
         \x20*\n\
         \x20*   A payload struct carries UPSTREAM'S LAYOUT. `X(obj)` is a call, not a\n\
         \x20*   cast, and answers a view the runtime owns. The view ALIASES real storage\n\
         \x20*   wherever zeo owns that storage as C-shaped memory, and is REFILLED from\n\
         \x20*   the object on every reach wherever it does not. A field zeo has no answer\n\
         \x20*   for is zero, and the accessor named for it raises.\n\
         \x20*\n\
         \x20* `rb_zeo_rdata` and `rb_zeo_rtypeddata` are the aliasing pair: the cell they\n\
         \x20* answer lives inside the object, so `RTYPEDDATA(v)->data = p` writes the\n\
         \x20* object's own slot and cannot go stale. Every other view is a refill, so a\n\
         \x20* pointer held across a call into Ruby reads what the object looked like at\n\
         \x20* the reach -- MRI gives the same warning about its own `RSTRING_PTR`.\n\
         \x20*\n\
         \x20* A refilled view does not write back. `RSTRING(s)->len = 3` changes the\n\
         \x20* view; the String keeps its length. The BYTES `RSTRING_PTR` answers are the\n\
         \x20* String's own, so writing through that pointer does reach it.\n\
         \x20*\n\
         \x20* `rb_zeo_ary_aset` is `RARRAY_ASET`, which must reach the Array: the pointer\n\
         \x20* `RARRAY_PTR_USE` hands out is a projection of tagged `VALUE`s built beside\n\
         \x20* the object, so a store through it stays in the projection.\n\
         \x20*\n\
         \x20* `rb_zeo_no_field` is the loud floor: a field zeo will not hand out. It\n\
         \x20* raises `NotImplementedError` naming the macro that reached it; the return\n\
         \x20* type only satisfies the caller.\n\
         \x20*/\n\
         #include \"ruby/internal/dllexport.h\"\n\
         #include \"ruby/internal/value.h\"\n\
         \n\
         struct RString;\n\
         struct RArray;\n\
         struct RObject;\n\
         struct RRegexp;\n\
         struct RMatch;\n\
         struct RFile;\n\
         struct RData;\n\
         struct RTypedData;\n\
         \n\
         RBIMPL_SYMBOL_EXPORT_BEGIN()\n\
         \n",
    );
    for (ret, name, params) in VIEW_ENTRIES {
        let sep = if ret.ends_with('*') { "" } else { " " };
        h.push_str(&format!("{ret}{sep}{name}({params});\n"));
    }
    h.push_str(
        "\nRBIMPL_SYMBOL_EXPORT_END()\n\
         \n\
         #endif /* RBIMPL_ZEO_H */\n",
    );
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list renders every entry, once, in C's spelling.
    #[test]
    fn every_entry_is_declared_once() {
        let h = render();
        for (_, name, _) in VIEW_ENTRIES {
            assert_eq!(h.matches(&format!("{name}(")).count(), 1, "{name}");
        }
        assert!(h.contains("struct RString *rb_zeo_rstring(VALUE obj);"));
        assert!(h.contains("VALUE rb_zeo_rbasic_class(VALUE obj);"));
        assert!(h.contains("void rb_zeo_ary_aset(VALUE ary, long i, VALUE v);"));
    }

    /// The list and the crate's definitions are the same set: an entry the
    /// runtime defines and the header does not declare is a symbol no
    /// extension can reach, and the reverse is a link error at load.
    #[test]
    fn the_list_is_the_crates_definitions() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut defined = std::collections::BTreeSet::new();
        for file in ["view.rs", "handles.rs", "io.rs"] {
            let text = std::fs::read_to_string(src.join(file)).expect("the source is present");
            for line in text.lines() {
                if let Some(rest) = line.trim_start().strip_prefix("fn rb_zeo_")
                    && let Some(end) = rest.find('(')
                {
                    defined.insert(format!("rb_zeo_{}", &rest[..end]));
                }
            }
        }
        let declared: std::collections::BTreeSet<String> =
            VIEW_ENTRIES.iter().map(|(_, n, _)| n.to_string()).collect();
        assert_eq!(declared, defined);
    }
}
