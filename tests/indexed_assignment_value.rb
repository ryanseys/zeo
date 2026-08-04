# An assignment expression evaluates to its RIGHT-HAND SIDE, never to what the
# setter returned. zeo answers the setter's return value.
#
# Ruby compiles `recv[i] = v` and `recv.x = v` to a send followed by a pop --
# the value left on the stack is always `v`, whatever `[]=` or `x=` answered.
# That is why a setter can `return false` and `h[k] = v` still evaluates truthy,
# and it is what makes chained assignment (`a = b[0] = 1`) give every target the
# same value.
#
# zeo emits the send and uses its result. For the builtin containers the two
# agree -- `Array#[]=` answers the value it stored -- so only a USER-defined
# setter shows the divergence, which is also where it matters most: a DSL whose
# `[]=` answers `self` for chaining silently changes the meaning of every
# assignment that uses it.

# (A setter cannot be written in the endless form -- ruby rejects
# `def []=(k, v) = ...` outright -- so these carry ordinary bodies.)
class Store
  def []=(k, v)
    :ignored_return
  end

  def value=(v)
    :also_ignored
  end
end

s = Store.new
p(s[:k] = 42)
p(s.value = 7)

x = (s[:k] = "chain")
p x

a = b = (s[:k] = 1)
p [a, b]

# The builtin path already agrees.
h = {}
p(h[:k] = 5)
