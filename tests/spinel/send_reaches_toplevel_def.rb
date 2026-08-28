# A top-level `def` is Object's PRIVATE instance method, so an explicit receiver
# cannot reach it -- except through #send, whose whole point is to ignore
# visibility. Spinel retargets `x.send(:m)` to a plain `x.m` in the analyzer,
# which is the right lowering but loses exactly the permission that made the
# call legal, so the top-level method was never found.
def zork(v) = "top:#{v}"
def plain = "top-plain"

class K
  def go = self.send(:zork, 3)
  def bare = send(:zork, 4)
  def noargs = self.send(:plain)
end

p K.new.go
p K.new.bare
p K.new.noargs
p send(:zork, 5)

o = Object.new
p o.send(:zork, 6)

# the receiver's own class still answers first
class Own
  def zork(v) = "class:#{v}"
  def go = self.send(:zork, 7)
end

p Own.new.go

# and send still reaches a private method of the receiver's own class
class Priv
  def call_it = self.send(:hidden)

  private

  def hidden = "private-hidden"
end

p Priv.new.call_it

# __send__ is the same protocol, and takes the same route now: the textual
# rewrite that consumed it before the analyzer could stamp it is gone
class Alias
  def go = self.__send__(:zork, 8)
  def bare = __send__(:zork, 9)
  def own = self.__send__(:mine)
  def hidden_call = self.__send__(:hidden)
  def mine = "class-mine"

  private

  def hidden = "class-hidden"
end

a = Alias.new
p a.go
p a.bare
p a.own
p a.hidden_call
p Alias.new.__send__("mine")

# still a BasicObject method, which is why the rewrite was blind to begin with
class Bare < BasicObject
  def go = __send__(:inner)
  def inner = "basic"
end

p Bare.new.go
