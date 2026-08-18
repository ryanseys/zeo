# The keyword/positional split rides on the trailing hash's OWN mark
# (`RHashData::kw_marked`, CRuby's `rb_keyword_given_p`), and a splat CLEARS
# it: `def forward(*args) = sink(*args)` hands `sink` a positional Hash, not
# keywords. `ruby2_keywords` is the opt-out, and the whole reason it exists.
def sink(a = 1, *rest, k: 2, **kw) = [a, rest, k, kw]

# Written keywords are keywords.
p sink(9, k: 3)

# A plain hash argument is positional...
h = { k: 3 }
p sink(9, h)
# ...and so is one a splat expanded, however it was written at the outer call.
def forward(*args) = sink(*args)
p forward(9, k: 3)

def forward_block(*args, &b) = sink(*args, &b)
p forward_block(9, k: 3)

# An explicit `**` at the forwarding site IS keywords again.
def forward_kw(*args, **kw) = sink(*args, **kw)
p forward_kw(9, k: 3)
p sink(9, **h)

# `...` forwards both channels intact.
def forward_all(...) = sink(...)
p forward_all(9, k: 3)

# `ruby2_keywords` keeps the mark, so the splat re-promotes.
def target(*a, **k) = [a, k]
ruby2_keywords def fwd(*a) = target(*a)
p fwd(1, k: 2)
p fwd(1, 2)
p fwd(1, { k: 2 })

class C
  def target(*a, **k) = [a, k]
  ruby2_keywords def fwd(*a) = target(*a)
  def plain(*a) = target(*a)
end
p C.new.fwd(1, k: 2)
p C.new.plain(1, k: 2)

# A bare `super` forwards keywords; an anonymous splat forwards positionally.
class A
  def m(a = 1, *rest, k: 2, **kw) = [a, rest, k, kw]
end
class B < A
  def m(*) = super
end
class D < A
  def m(a = 1, *rest, k: 2, **kw) = super
end
p B.new.m(9, k: 3, z: 4)
p D.new.m(9, k: 3, z: 4)

# `Proc#ruby2_keywords` answers the proc.
pr = proc { |x| x }
p pr.ruby2_keywords.equal?(pr)
