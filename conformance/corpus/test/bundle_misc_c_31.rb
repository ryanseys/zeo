# Bundled tests:
#   - str_rindex_optional_narrow
#   - string_new_array_literal
#   - string_split_with_limit

# === str_rindex_optional_narrow ===
def t_str_rindex_optional_narrow
  # Issue #645: `slash = p.rindex("/")` followed by a nil-check
  # ternary used to leak sp_RbVal into sp_str_sub_range_r (which
  # expects mrb_int):
  #
  #   tail = slash == nil ? p : p[(slash + 1)..(p.length - 1)]
  #
  # Root cause: spinel widened `String#rindex` return to "poly"
  # unconditionally (sp_str_rindex_poly returns sp_RbVal), even
  # when the arg was a plain string. `slash + 1` then went through
  # sp_poly_add and the result couldn't pass as the mrb_int start
  # arg.
  #
  # Fix: split rindex like index — plain-string arg returns int?
  # via sp_str_rindex_opt (SP_INT_NIL sentinel), regex arg stays
  # on sp_re_rindex_poly. The nil-check narrow (#645 infra) then
  # applies.
  
  p = "abc/def"
  slash = p.rindex("/")
  tail = slash == nil ? p : p[(slash + 1)..(p.length - 1)]
  puts tail
  
  # not-found case
  p2 = "no_slash_here"
  slash2 = p2.rindex("/")
  tail2 = slash2 == nil ? p2 : p2[(slash2 + 1)..(p2.length - 1)]
  puts tail2
  
  # multiple separators — rindex finds the last
  p3 = "a/b/c/d"
  puts p3.rindex("/").inspect
  slash3 = p3.rindex("/")
  puts slash3 == nil ? "none" : p3[(slash3 + 1)..(p3.length - 1)]
  
  # zero-position match
  puts "/foo".rindex("/")
end
t_str_rindex_optional_narrow

# === string_new_array_literal ===
def t_string_new_array_literal
  # #519. `[String.new]` (and `[s]` where `s = String.new`) used to
  # infer as `int_array`, then codegen emitted
  # `sp_IntArray_push(arr, sp_String_new(""))` -- C type mismatch.
  #
  # Fix: `infer_array_elem_type_from_ids` now has a `mutable_str` arm
  # that lowers the literal to `mutable_str_ptr_array` (a sp_PtrArray
  # of sp_String*). The generic `<X>_ptr_array` codegen path handles
  # length / push / pop / [] for the new slot.
  #
  # `["", String.new]` (mixed string-literal + mutable_str) still
  # widens to poly_array because the literal witness flips et to
  # "string" first and the all-string check fails.
  
  arr = [String.new]
  puts arr.length
  
  s = String.new
  arr2 = [s]
  puts arr2.length
  
  # Mixed: literal + String.new -> poly_array. Both elements show up.
  mixed = ["", String.new]
  puts mixed.length
end
t_string_new_array_literal

# === string_split_with_limit ===
def t_string_split_with_limit
  # String#split with a positive `limit` argument caps the result at
  # `limit` elements; the last element keeps the unsplit remainder.
  # Pre-fix spinel's two-arg split fell through to sp_str_split, which
  # ignored the limit and split exhaustively. Issue #619 puzzle 2.
  p "hi!".split("", 2) == ["h", "i!"]
  p "a,b,c,d".split(",", 2) == ["a", "b,c,d"]
  p "a,b,c,d".split(",", 3) == ["a", "b", "c,d"]
  p "abc".split("", 1) == ["abc"]                     # limit=1 keeps the whole string
  p "abc".split("", 3) == ["a", "b", "c"]             # limit == #chars, exact split
  p "a,b,c".split(",", -1) == ["a", "b", "c"]         # negative limit -> full split
end
t_string_split_with_limit

