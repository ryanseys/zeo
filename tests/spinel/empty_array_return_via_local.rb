# An empty array literal returned from a method through a LOCAL variable lost
# its container type and settled at String: the method was emitted as
# `sp_String *lv_a = sp_String_new_shared(sp_IntArray_new())`, and every use of
# the result in the caller followed it -- `<<` became sp_String_append_bin, so a
# Float or an object arrived at a `const char *` parameter.
#
# Returning the very same literal directly (`def make_direct; []; end`) was
# always fine, which is what kept this hidden.

def make
  a = []
  a
end

xs = make
xs << 1.5
p xs
p xs.length

def make_direct
  []
end

ys = make_direct
ys << 1.5
p ys

# push instead of << also stayed correct: only << is ambiguous between Array
# and String, so only << followed the mistyped literal.
def make_push
  a = []
  a
end

zs = make_push
zs.push(1.5)
p zs

# The shape this was found in: a generic top-K helper whose result is assigned
# back over its argument, with the accumulator seeded from a bare [].
class Item
  attr_reader :score
  def initialize(s)
    @score = s
  end
end

def top1(kept)
  trimmed = []
  trimmed << kept[0]
  trimmed
end

items = []
items << Item.new(3.0)
items << Item.new(1.0)
items = top1(items)
p items.length
p items[0].score

# The other side of the gate (#3227 P6): when the caller's slot really is a
# String, mutating the result of a method whose tail is a local still has to
# pull that local into the shared-handle set, so the mutation lands on the one
# buffer and aliases of it see the change.
def make_held
  s = +"x"
  s << "y"
  s
end

r = make_held
r << "z"
p r

def mk2
  a = +"p"
  a
end

q = mk2
q2 = q
q << "!"
p q
p q2
p q.equal?(q2)
