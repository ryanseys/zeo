# #526. Sibling to #522. #522 fixed `.map { String.new }` block-return
# inference so the result is correctly typed as
# `mutable_str_ptr_array` (a sp_PtrArray of sp_String*). But the
# is_ptr_array_type recv block in codegen had no `join` arm for this
# variant — `xs.map { String.new }.join(",")` fell through to the
# unresolved-call warning and emitted 0 at the call site (compile-time
# warning, runtime no-op producing empty output between separators in
# the composite JSON-fragment use case in action_view/view_helpers).
#
# Fix: add the `join` arm in the ptr_array recv block (gated on
# elem_type == "mutable_str") that emits sp_PtrArray_str_join, and the
# matching runtime helper that reads each element's bytes via the
# sp_String data/len pair.

def make_str(x)
  s = String.new
  s << x.to_s
  s
end

xs = [1, 2, 3]

# The repro case from the issue: block returns String.new directly.
puts xs.map { |x| String.new }.join(",")

# Block body is a method call returning a mutable_str — mirrors the
# real-world action_view/view_helpers pattern where the map block calls
# something like `Views::Articles.article_json(article)` and the result
# is joined with "," to form a JSON fragment.
puts xs.map { |x| make_str(x) }.join("-")

# Single-element (no separator emitted in output).
puts [1].map { |x| make_str(x) }.join(",")
