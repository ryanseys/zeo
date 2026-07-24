# `v.gsub(/regex/, "repl")` where v is poly-typed (sp_RbVal, e.g.
# from a poly-valued hash lookup) used to emit an empty SP_TAG_OBJ
# dispatch shell — the per-tag arm was missing — leaving the result
# temp at its default mrb_int 0 and the downstream const char *
# assignment failing C compile.
#
# compile_poly_method_call now emits a SP_TAG_STR arm for gsub /
# sub at the same level as the existing SP_TAG_INT `[]` bit-extract.
# poly_dispatch_return_type was also extended to report "string"
# for these methods so the result temp is correctly typed (and the
# arm's assignment doesn't try to store const char * into a
# mrb_int slot).

def normalize(entry)
  v = entry[:stream]
  v.gsub(/[^a-z]/, "_")
end

# Force the hash to be poly-valued by mixing value types — the
# `:stream` value is String, but `:count` is Int, so the analyzer
# widens the hash to sym_poly_hash and `entry[:stream]` returns
# sp_RbVal rather than const char *.
h = { stream: "abc/def", count: 5 }
puts normalize(h)
