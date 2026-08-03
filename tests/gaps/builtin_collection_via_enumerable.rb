# zeo implements Array's collection methods by routing to `Enumerable`, whose
# bodies iterate by SENDING `each` (`builtins/enumerable.rs`'s `for_each`).
# CRuby defines them on Array itself in C -- `rb_ary_collect` walks the array's
# storage directly and never calls `each` at all.
#
# Invisible until `each` could be overridden at runtime, which
# tests/issue_runtime_override_builtin_iterator.rb just made possible: patching
# `Array#each` now also changes `map`, `select`, `sum` and the rest, where ruby
# leaves every one of them alone.
#
# ARRAY ONLY. `Hash#map` and `Range#map` really are `Enumerable#map` in CRuby
# too, and really do follow a patched `each` there -- oracle-verified, and the
# reason this file does not test them. So the fix is 18 `inherited_row!
# (enumerable, ...)` rows on Array, each needing a direct implementation over
# the receiver's own storage. That is what CRuby has, and it would be faster
# besides. The `inherited_row!` mechanism itself is right for reflection --
# `Array.instance_method(:map).owner` must still answer `Array` -- so the rows
# stay and only the bodies change.

a = [4, 5]
Array.send(:define_method, :each) { |&b| b.call(:patched); self }

# `each` itself is patched -- the control line, and it agrees.
seen = []
a.each { |v| seen << v }
p seen

# ...and none of these should be.
p a.map { |v| v * 2 }
p a.select { |v| v > 4 }
p a.sum
p a.to_a
p a.include?(5)
p a.count
p a.min
