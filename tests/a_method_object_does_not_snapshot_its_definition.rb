# A `Method` or `UnboundMethod` captured before a reopen keeps answering for
# the definition it was taken from.
#
# The BODY half was already right: `RMethod::snapshot` froze the entry, so
# `m.call` ran the captured body. What followed the live tables was the
# METADATA -- `#arity`, `#parameters`, `#source_location` and `#inspect` are
# keyed by `(class, name)` with no position, and a redefinition timeline
# re-registers that key at each body's own line. So one handle described two
# different definitions depending on which question you asked.
#
# The fix is the metadata twin of the entry snapshot: the handle captures its
# `Arc<MethodMeta>` at construction and every reflection row reads through it.
# CRuby holds the `rb_method_entry_t` it was built from, which makes all of
# these a snapshot rather than a lookup.
#
# `meta` is `None` exactly where `snapshot` is None, and for the same reason:
# a `#super_method` re-seat names a position the ordinary walk would not
# reach, so it resolves afresh. `#bind` and `#unbind` carry the row on, since
# `um.arity` and `um.bind(o).arity` describe one definition.
class U
  def u = 1
end
first = U.instance_method(:u)
class U
  def u(a) = 2
end
p [first.arity, U.instance_method(:u).arity]
p first.parameters
p U.instance_method(:u).parameters

class B
  def b = "b1"
end
m = B.new.method(:b)
class B
  def b = "b2"
end
p [m.arity, m.call, B.new.b]

# The sweep.

# `source_location` and `inspect` follow the same row as `arity`.
class S
  def s = 1
end
h = S.instance_method(:s)
line_before = h.source_location[1]
class S
  def s(a, b) = 2
end
p [h.arity, h.parameters, h.source_location[1] == line_before]
p [S.instance_method(:s).arity, S.instance_method(:s).source_location[1] == line_before]

# `#bind` carries the row: the bound Method must describe the same definition
# the UnboundMethod does, not the live one.
class BN
  def m = 1
end
u = BN.instance_method(:m)
class BN
  def m(a, b, c) = 2
end
p [u.arity, u.bind(BN.new).arity, BN.new.method(:m).arity]

# `#unbind` carries it the other way.
class UB
  def m = 1
end
bm = UB.new.method(:m)
class UB
  def m(a) = 2
end
p [bm.arity, bm.unbind.arity, UB.instance_method(:m).arity]

# `dup` carries it.
class DP
  def m = 1
end
d = DP.instance_method(:m)
class DP
  def m(a) = 2
end
p [d.dup.arity, d.arity]

# A handle taken AFTER the last body describes that body -- the ordinary case,
# and the one a snapshot must not break.
class AF
  def m = 1
end
class AF
  def m(a, b) = 2
end
p [AF.instance_method(:m).arity, AF.new.method(:m).arity]

# A method with no redefinition at all is untouched.
class NR
  def m(a, b = 1, *r, k:, **kw, &blk) = nil
end
p NR.instance_method(:m).parameters
p NR.instance_method(:m).arity
