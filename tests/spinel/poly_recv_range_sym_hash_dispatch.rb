# Two fixes for poly-receiver dispatch on built-in container types
# from issues #588 and #590.
#
# #588: `case x; when Range; r.include?(x); end` -- the poly-recv
# include? dispatch enumerated IntArray / IntStrHash cls_ids but
# missed SP_BUILTIN_RANGE, so a Range narrowed via case/when fell
# through to the default `0` (always-false). `sp_range_include` was
# added in lib/sp_runtime.h and the codegen dispatch arm picks it
# up.
#
# #590: A sym_*_hash lookup with a poly-typed key (e.g. an
# `is_a?(Symbol)`-narrowed local that still carries sp_RbVal at C
# level) emitted `sp_SymPolyHash_get(h, lv_x)` where the parameter
# expects `sp_sym`. The `compile_arg0_as_sym` helper now extracts
# `.v.i` for poly args; the sym_int_hash / sym_str_hash arms also
# reuse it, and the prior "non-symbol key -> fallback default"
# fast-path no longer fires for poly args (those CAN be symbols at
# runtime).
#
# Also extends `sp_poly_inspect` to dispatch built-in container
# cls_ids (Range, Time, typed Arrays) instead of returning the bare
# "#<Object>" placeholder -- the #590 minimal repro stored a Range
# inside a sym_poly_hash, and `result.inspect` printed "#<Object>"
# instead of "200..299".

# --- #588: case/when narrowing to Range, include? via poly recv.
def check_range(value, actual)
  case value
  when Range
    value.include?(actual)
  when Integer
    value == actual
  else
    false
  end
end

puts check_range(200..299, 200)   # true
puts check_range(200..299, 300)   # false
puts check_range(200..299, 250)   # true
puts check_range(404, 404)        # true
puts check_range(404, 200)        # false

# --- #590: sym-keyed hash with poly-typed key extracted from a
# Symbol|Integer narrowing.
H = { a: 200..299, b: 404, c: 500 }

def lookup(expected)
  expected.is_a?(Symbol) ? H[expected] : expected
end

puts lookup(:a).inspect           # 200..299  (was "#<Object>" pre-fix)
puts lookup(:b).inspect           # 404
puts lookup(:c).inspect           # 500
puts lookup(503).inspect          # 503
