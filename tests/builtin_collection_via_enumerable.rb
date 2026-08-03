# A collection method CRuby defines on the class itself walks that class's own
# storage -- `rb_ary_collect` never calls `each`. zeo routed 23 of these rows
# through `Enumerable`, whose bodies iterate by SENDING `each`, so overriding
# `each` at runtime changed all of them at once.
#
# Invisible until a runtime override of a builtin iterator could take effect
# (tests/issue_runtime_override_builtin_iterator.rb). The `inherited_row!`
# rows stay -- reflection needs `Array.instance_method(:map).owner` to answer
# `Array` -- and only the bodies changed.
#
# The rows that really ARE Enumerable's in CRuby (`Range#map`, `Hash#map`) are
# deliberately absent: they follow a patched `each` in ruby too.

a = [4, 5, 6]
Array.send(:define_method, :each) { |&b| b.call(:patched); self }

# `each` itself is patched -- the control line.
seen = []
a.each { |v| seen << v }
p seen

# ...and nothing below is, because Array owns every one of these.
p a.map { |v| v * 2 }
p a.collect { |v| v - 1 }
p a.select { |v| v > 4 }
p a.filter { |v| v.even? }
p a.reject { |v| v > 4 }
p a.find { |v| v > 4 }
p a.detect { |v| v > 5 }
p a.all? { |v| v > 3 }
p a.any? { |v| v > 5 }
p a.none? { |v| v > 9 }
p a.one? { |v| v == 5 }
p a.count
p a.count(5)
p a.count { |v| v > 4 }
p a.sum
p a.sum(10)
p a.max
p a.max(2)
p a.min
p a.min(2)
p a.minmax
p a.take(2)
p a.drop(1)
p a.take_while { |v| v < 6 }
p a.drop_while { |v| v < 6 }
rv = []
a.reverse_each { |v| rv << v }
p rv

# The argument-less predicates read truthiness, not a block.
p [nil, false].any?
p [nil, 1].any?
p [].all?
p [1, nil].none?
p [nil, 1].one?

# Range's four, and Hash's one.
r = (1..4)
Range.send(:define_method, :each) { |&b| b.call(:rpatched); self }
p r.count
p r.minmax
rr = []
r.reverse_each { |v| rr << v }
p rr

h = { x: 1, y: 2 }
Hash.send(:define_method, :each) { |&b| b.call(:hpatched, nil); self }
p h.any?
p h.any? { |k, v| v > 1 }
p({}.any?)
