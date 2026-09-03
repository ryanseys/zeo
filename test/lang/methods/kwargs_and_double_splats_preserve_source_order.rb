# The KwArg merge: `Call`/`HashLit`/`Yield` carry ONE ordered list of
# literal pairs and `**h` double-splats. Ruby's Hash is insertion-ordered
# and the merge is left-to-right last-wins, so the interleaving is
# observable. The old two-field split got the order wrong for a splat
# before a pair and couldn't represent two splats at all. All
# oracle-verified against ruby 4.0.6.

def capture(**h) = h
p capture(**{a: 1, b: 2}, c: 3)   # splat before pair
p capture(c: 3, **{a: 1, b: 2})   # pair before splat
p capture(**{x: 1}, **{y: 2})     # two splats
p capture(**{x: 1}, **{x: 2})     # last-wins on dup key
p capture(**{a: 1}, b: 2, **{c: 3})
h = {a: 1}
p({**h, b: 2})
p({b: 2, **h})
def y
  yield 1, **{k: 2, j: 3}
end
y { |*a| p a }
class HasToHash
  def to_hash = {z: 99}
end
p capture(**HasToHash.new, w: 1)
__END__
{a: 1, b: 2, c: 3}
{c: 3, a: 1, b: 2}
{x: 1, y: 2}
{x: 2}
{a: 1, b: 2, c: 3}
{a: 1, b: 2}
{b: 2, a: 1}
[1, {k: 2, j: 3}]
{z: 99, w: 1}
#@ stderr
lang/methods/kwargs_and_double_splats_preserve_source_order.rb:12: warning: key :x is duplicated and overwritten on line 12
