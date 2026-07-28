# `recv&.meth(*args)` -- safe navigation combined with a splat argument. zeo's
# splat call path has no `&.` short-circuit, so this is a clean compile error
# where ruby returns the call's value (and nil for a nil receiver). Blocks
# webrick, whose config dispatch is `@config[cb]&.call(*args)`.
def add(a, b)
  a + b
end

args = [1, 2]
recv = method(:add)
p recv&.call(*args)

recv = nil
p recv&.call(*args)
