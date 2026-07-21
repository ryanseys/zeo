# Issue #378: `case <string> when :sym_literal` (and the
# symmetric `case <symbol> when "string_literal"`) used to lower
# to `strcmp(str, SPS_<sym>) == 0` / `mrb_int == const char *` —
# C type errors on every such arm because `compile_expr` on a
# SymbolNode returns the `sp_sym` numeric id (a long long), and
# strcmp expects `const char *`.
#
# CRuby semantics: `:sym === "string"` is false (Symbol#=== only
# matches Symbols), and `"string" === :sym` is also false. The
# fix emits a literal `0` for these cross-type when arms — the
# C compiles, and the runtime pick correctly falls through to
# the else clause to match CRuby.

def lookup(name)
  case name
  when :id
    1
  when :body
    2
  else
    -1
  end
end

# String case-expr against symbol when-arms — every arm misses,
# else clause runs.
puts lookup("id")     # -1 (not 1)
puts lookup("body")   # -1 (not 2)
puts lookup("nope")   # -1

def reverse(sym)
  case sym
  when "id"
    1
  when "body"
    2
  else
    -1
  end
end

# Symbol case-expr against string when-arms — symmetric: every
# arm misses, else runs.
puts reverse(:id)     # -1
puts reverse(:body)   # -1

# Sanity: matching arms still work.
def lookup_sym(s)
  case s
  when :id
    1
  when :body
    2
  else
    -1
  end
end
puts lookup_sym(:id)   # 1
puts lookup_sym(:body) # 2
