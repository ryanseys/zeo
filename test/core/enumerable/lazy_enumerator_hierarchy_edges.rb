# `Enumerator::Lazy < Enumerator` beyond the promoted gap shape: ancestry
# answers, lazy zip semantics, and the inherited terminals.

lz = (1..3).lazy

# The reparent is visible to every ancestry question.
p [lz.is_a?(Enumerator), lz.is_a?(Enumerable), lz.kind_of?(Enumerator::Lazy)]
p(case lz when Enumerator then :enumerator_case end)

# zip stays lazy on an ENDLESS receiver and pads with nil past a shorter
# other's end.
pairs = (1..Float::INFINITY).lazy.zip(%w[a b]).first(3)
p pairs

# zip size is the receiver's size.
p (1..4).lazy.zip([9, 9]).size

# zip composes with later links, still lazily.
p (1..Float::INFINITY).lazy.zip([10, 20, 30]).map { |a, b| [a, b] }.first(2)

# A block makes zip EAGER (the Enumerable row), answering nil like CRuby.
got = []
r = [1, 2].lazy.zip([3, 4]) { |pair| got << pair }
p [r, got]

# The inherited terminals drive the chain through Enumerator#each.
doubled = (1..4).lazy.map { |x| x * 2 }
p doubled.to_a
p doubled.entries
p doubled.first(2)
p doubled.force

# External iteration through the Enumerator rows, with a rewind restart.
ext = (5..).lazy.select(&:even?)
p [ext.next, ext.peek, ext.next]
ext.rewind
p ext.next

# `eager` hands back a plain Enumerator over the same sequence.
e = (1..3).lazy.map { |x| x + 1 }.eager
p [e.class, e.to_a]

# to_s stays the address form while inspect prints the chain.
s = (1..2).lazy.map(&:to_s)
p s.to_s.sub(/0x[0-9a-f]+/, "0xADDR")
p s.inspect.include?("map")
__END__
[true, true, true]
:enumerator_case
[[1, "a"], [2, "b"], [3, nil]]
4
[[1, 10], [2, 20]]
[nil, [[1, 3], [2, 4]]]
[2, 4, 6, 8]
[2, 4, 6, 8]
[2, 4]
[2, 4, 6, 8]
[6, 8, 8]
6
[Enumerator, [2, 3, 4]]
"#<Enumerator::Lazy:0xADDR>"
true
