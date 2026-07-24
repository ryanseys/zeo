# String#slice(Range) — decompose the Range and reuse the same
# runtime helper as String#[](Range). Inclusive, exclusive,
# endless, beginless and negative endpoints all supported.
p "hello".slice(1..3)
p "hello".slice(1...3)
p "hello".slice(1..)
p "hello".slice(..2)
p "hello".slice(1..-2)
p "hello".slice(2..10)
p "hello".slice(0..-1)

# slice still works with (start, length) and single-index forms.
p "hello".slice(1, 2)
p "hello".slice(1)

# Through a local receiver too.
s = "spinel"
p s.slice(2..4)
