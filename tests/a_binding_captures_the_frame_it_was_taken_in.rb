# `Kernel#binding` cannot be a builtin row: it would have to see its CALLER's
# locals. The emitter builds it instead, from the names a `binding` in the
# scope promotes to cells -- the only storage a binding can share.

a = 1
b = "two"
bd = binding
p bd.local_variables.sort
p bd.local_variable_get(:a)
bd.local_variable_set(:a, 99)
p a
p bd.eval("a.to_s + b")
p bd.receiver
p bd.local_variable_defined?(:b)
p bd.local_variable_defined?(:nope)

# A method's binding reports its params and its own locals, and nothing of
# the caller's.
def m(x, y = 2)
  z = x + y
  binding
end
mb = m(5)
p mb.local_variables.sort
p mb.eval("x + y + z")

# A block's binding reports the block's own names FIRST, then the enclosing
# scope's -- CRuby's innermost-first order -- and a name the block binds
# shadows the outer one.
outer_only = :outer
shadowed = :from_main
seen = nil
[10].each do |item|
  shadowed = :from_block
  inner = item * 2
  seen = binding.local_variables
end
p seen.sort
p shadowed

# A destructuring block parameter binds the names it destructures, and zeo's
# own internal slot for the un-destructured value is not one of them.
names = nil
[[[1, 2], 9]].each do |(x, y), i|
  names = binding.local_variables.sort
end
p names

# `Proc#binding` is the scope the proc was BUILT in -- its own locals do not
# exist until it runs.
def make
  hidden = :in_make
  proc { hidden }
end
pr = make
p pr.binding.local_variables.sort
p pr.binding.local_variable_get(:hidden)
p pr.binding.eval("hidden")

# A lambda too.
lam_local = :outside
lam = lambda { |q| q }
p lam.binding.local_variable_defined?(:lam_local)

# Assignment through a binding is visible to the frame it came from.
def round_trip
  v = 1
  bd = binding
  bd.local_variable_set(:v, 2)
  v
end
p round_trip
