# Conditional-assign of a poly value (e.g. nullable hash lookup
# fused with a nil-guard fallback) leaves the local typed sp_RbVal
# even when both branches produce strings. Downstream concat and
# print sites must coerce on the sp_RbVal -> const char * boundary
# rather than fall through to a struct-into-pointer cc error.
#
#   * `"a" + poly + "b"` -- chained concat collapses to
#     sp_str_concat3; collect_concat_parts now wraps poly with
#     sp_poly_to_s (mirrors the existing 2-arg sp_str_concat
#     coercion at the single-`+` site).
#   * `print poly` -- compile_print routes through sp_poly_to_s
#     instead of the broken `printf("%lld", (long long)val)`
#     fallback (sp_RbVal is a struct; the cast rejects).
#
# str_poly_hash lookup is the canonical shape that triggers the
# poly local: { "text" => "hi", "n" => 42 }["text"] is sp_RbVal
# because the value union spans String and Integer.

def show_concat(node)
  raw = node["text"]
  text = if raw.nil?
           ""
         else
           raw
         end
  puts "[ " + text + " ]"
end

def show_print(node)
  raw = node["text"]
  text = if raw.nil?
           ""
         else
           raw
         end
  print "[ "
  print text
  print " ]\n"
end

# --- Present-key String value (the canonical bug-report shape).
# Mixed-value hash -> str_poly_hash; the else branch produces a poly
# value (raw), the if branch produces a string literal. The union of
# the two branches stays sp_RbVal. Without the codegen-side coercion,
# the downstream concat / print sites emit cc-rejected C.
h_present = { "text" => "hello", "n" => 42 }
show_concat(h_present)
show_print(h_present)

# --- Missing-key fallback. Exercises the if-true branch of the
# nil-guard: raw is nil, text becomes "", concat / print emit
# "[  ]" / "[  ]\n". h_missing keeps the same str_poly_hash shape
# (mixed String + Integer values so the value union widens to
# poly) but omits the "text" key so the lookup returns nil.
# Without this case the if-branch of the conditional assignment
# was untested.
h_missing = { "other" => "x", "n" => 42 }
show_concat(h_missing)
show_print(h_missing)

# --- Symbol-valued poly via `print`. CRuby's `print :sym` calls
# `Symbol#to_s` -> "sym"; spinel's compile_print poly arm routes
# through `sp_poly_to_s` whose SP_TAG_SYM case returns the same
# rendering. Exercises the tag dispatch beyond SP_TAG_STR.
def show_print_sym(node)
  raw = node["k"]
  text = raw.nil? ? "" : raw
  print "sym="
  print text
  puts ""
end
h_sym = { "k" => :hello, "n" => 42 }
show_print_sym(h_sym)

# --- Integer-valued poly via `print`. SP_TAG_INT path through
# `sp_poly_to_s` returns `sp_int_to_s(...)`; CRuby's `print 42`
# calls `Integer#to_s`. (Skipped for concat: CRuby raises
# TypeError on `String + Integer`; the coercion path stays
# behind `print` to preserve parity.)
def show_print_int(node)
  raw = node["k"]
  text = raw.nil? ? "" : raw
  print "int="
  print text
  puts ""
end
h_int = { "k" => 42, "n" => "x" }
show_print_int(h_int)

# --- Explicit 4-part chained concat that exercises sp_str_concat4.
# Uses bound prefix/suffix locals (rather than bare string literals)
# so the part shape is unambiguous regardless of any literal-folding
# optimization pass.
def label_concat4(node)
  raw = node["text"]
  text = raw.nil? ? "" : raw
  prefix = "OPEN"
  suffix = "CLOSE"
  puts prefix + " " + text + " " + suffix
end
label_concat4(h_present)
label_concat4(h_missing)

# --- Variable-length sp_str_concat_arr (>= 5 parts) with bound
# prefix / suffix to ensure the chain stays unambiguous.
def label_wrap(node)
  raw = node["text"]
  text = raw.nil? ? "" : raw
  pre1 = "<"
  pre2 = "open "
  post1 = " close"
  post2 = ">"
  puts pre1 + pre2 + text + post1 + post2
end
label_wrap(h_present)
