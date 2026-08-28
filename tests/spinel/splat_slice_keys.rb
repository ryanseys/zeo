# `Hash#slice(*keys)` is variadic, but `slice` sat in the fixed-arity splat
# expansion table with arity 1, so a splat whose length is not a literal was
# rewritten to `keys[0]` -- every key after the first silently dropped. A
# constant splat was not expanded at all and reached the emitter as one whole
# array, which the key coercion answered nothing for. Three spellings of one
# splat gave three answers (#4164).
ATTRS = [:a, :b]
IDX = [1, 2]
h = { a: 1, b: 2, c: 3 }
x = ATTRS
y = IDX

def poly(n) = n > 0 ? { a: 1, b: 2, c: 3 } : 7
def polys(n) = n > 0 ? "abcdef" : 7
def polya(n) = n > 0 ? [10, 20, 30, 40] : 7

p h.slice(*[:a, :b]).keys
p h.slice(*ATTRS).keys
p h.slice(*x).keys
p h.slice(:a, *[:b]).keys
p h.slice(*[]).keys
p h.except(*ATTRS).keys
p h.values_at(*ATTRS)

# the same splat on a BOXED receiver: Hash#slice answers a sub-Hash, while
# String#slice / Array#slice is #[] and takes one argument or two -- only the
# run time says which, so the key list decides
p poly(1).slice(*ATTRS).keys
p poly(1).values_at(*ATTRS)
p polys(1).slice(*IDX)
p polya(1).slice(*IDX)
p polys(1).slice(*[1])
