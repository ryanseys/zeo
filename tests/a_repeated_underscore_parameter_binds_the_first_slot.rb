# Ruby lets a parameter name repeat when it begins with `_` -- `def f(a, a)` is
# a SyntaxError, `def f(_, _)` is not -- and only the FIRST occurrence owns the
# readable local. Every shape below is oracled against ruby 4.0.6.
#
# The method signature and the block prologue got this wrong in opposite ways:
# a method put one Rust ident on two parameters (rustc E0415, which is where
# aws-sdk-core stopped), while a block's sequential bindings merely shadowed,
# so it compiled and quietly answered the LAST argument.

def two(_, _)
  _
end
p two(1, 2)

def lead_and_post(*_, _)
  _
end
p lead_and_post(1, 2, 3)

def every_slot(_, *_, **_, &_)
  _
end
p every_slot(1, 2)

def reassigned(_, _)
  _ = 99
  _
end
p reassigned(1, 2)

# A `_` parameter is still an ordinary local, so the later slots being
# unreachable by name costs the body nothing it could have used.
def counts(_, _)
  [1, 2, 3].map { |_| _ * 2 }
end
p counts(1, 2)

p proc { |_, _| _ }.call(7, 8)
p lambda { |_, _| _ }.call(7, 8)

# The spliced iterator path binds its params as sequential `let`s too.
p [[1, 2], [3, 4]].map { |_, _| _ }
p({ a: 1, b: 2 }.map { |_, _| _ })
{ a: 1 }.each { |_, _| p _ }

# A destructuring group assigns BY NAME, so its writes land on the slot the
# name owns and the last one wins -- unlike a repeated parameter slot.
p proc { |_, (_, _)| _ }.call(1, [2, 3])
