# Issue #833 (#801 partial): *StrHash missing-key bracket access
# returns nil per MRI, not the type's zero value.
# *IntHash variants still return 0 (their analyze-side LV slot
# can't be widened to int? without cascading through declare_var --
# deferred to a follow-up phase).

# SymStrHash
puts({a: "hi"}[:missing].inspect)

# StrStrHash
puts({"a" => "hi"}["missing"].inspect)

# IntStrHash
puts({1 => "hi"}[99].inspect)

# Existing key still works
puts({a: "hello"}[:a])
