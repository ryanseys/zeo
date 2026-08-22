# Ruby installs each `def` WHERE IT STANDS, so a call written between two
# same-name definitions answers the FIRST body. zeo's static tables carry only
# the last-def-wins winner, so `analyze::redefs` puts the timeline in the
# runtime overlay instead: the first body installs at boot, and each later one
# re-installs at its own document position.
#
# The instance channel had that; the class-method channel did not -- `redefs`
# skipped `is_class_method` outright, so `C.t` answered the reopened body from
# the program's first line. The two channels are separate maps and separate
# hooks, which is what this file sweeps.
#
# No call-site change was needed: a class-method send is already dynamic (it
# has an explicit receiver), and the overlay outranks the frozen row. What was
# missing was the install.

puts "== a reopen across class bodies"
class C
  def self.t = "t1"
end
p C.t
class C
  def self.t = "t2"
end
p C.t

puts "== module_function reaches it the same way"
module F
  module_function
  def g = "g1"
end
p F.g
module F
  module_function
  def g = "g2"
end
p F.g

puts "== three bodies, and every window sees its own"
class Three
  def self.v = 1
end
a = Three.v
class Three
  def self.v = 2
end
b = Three.v
class Three
  def self.v = 3
end
p [a, b, Three.v]

puts "== two definitions in ONE body with a statement between"
class Between
  def self.w = "first"
  SEEN = w
  def self.w = "second"
end
p [Between::SEEN, Between.w]

puts "== the two channels do not interfere"
class Both
  def self.n = "cls1"
  def n = "inst1"
end
p [Both.n, Both.new.n]
class Both
  def self.n = "cls2"
  def n = "inst2"
end
p [Both.n, Both.new.n]

puts "== a redefined class method keeps its arguments, block and keywords"
class Args
  def self.k(a, b = 2, *rest, c:, d: 4, &blk) = "1:#{a},#{b},#{rest},#{c},#{d},#{blk&.call}"
end
p Args.k(1, c: 3) { "blk" }
class Args
  def self.k(a, b = 2, *rest, c:, d: 4, &blk) = "2:#{a},#{b},#{rest},#{c},#{d},#{blk&.call}"
end
p Args.k(1, 9, 8, c: 3, d: 7) { "blk" }

puts "== self is the CLASS in every body: class-level ivars and cvars"
class SelfIsClass
  @tag = "ivar"
  @@shared = "cvar"
  def self.s = "1:#{@tag}/#{@@shared}"
end
p SelfIsClass.s
class SelfIsClass
  def self.s = "2:#{@tag}/#{@@shared}"
end
p SelfIsClass.s

puts "== super from a redefined class method"
class SuperBase
  def self.s = "base"
end
class SuperSub < SuperBase
  def self.s = "1(#{super})"
end
p SuperSub.s
class SuperSub
  def self.s = "2(#{super})"
end
p SuperSub.s

puts "== a subclass sees the timeline of the parent's class method"
class PBase
  def self.p1 = "p1"
end
class PSub < PBase; end
before = PSub.p1
class PBase
  def self.p1 = "p2"
end
p [before, PSub.p1]

puts "== a module's own def self.x"
module ModSelf
  def self.m = "m1"
end
p ModSelf.m
module ModSelf
  def self.m = "m2"
end
p ModSelf.m

puts "== reflection names the owner, and the last body's report"
# `#arity`/`#parameters` in the WINDOW report the FINAL body on both channels
# -- the meta rows are static and the timeline lives in the overlay. See
# `tests/gaps/a_redefinition_window_reports_the_last_body.rb`.
class Reflect
  def self.r = "r1"
end
p [Reflect.method(:r).owner.to_s, Reflect.r]
class Reflect
  def self.r(x) = "r2#{x}"
end
p [Reflect.method(:r).owner.to_s, Reflect.method(:r).arity, Reflect.r("!")]

puts "== singleton_method_added reports each definition where it stands"
class Hooked
  def self.singleton_method_added(n)
    (@seen ||= []) << n unless n == :singleton_method_added
    super
  end
  def self.a = "a1"
end
p Hooked.instance_variable_get(:@seen)
class Hooked
  def self.a = "a2"
end
p [Hooked.instance_variable_get(:@seen), Hooked.a]

puts "== a redefinition clears the private mark, as ruby does"
# Only the SECOND half is asserted here: a `private_class_method` mark made
# BEFORE the reopen is not enforced inside the window -- the same on both
# channels, and the same gap file as the arity above.
class Vis
  def self.v = "v1"
  private_class_method :v
end
class Vis
  def self.v = "v2"
end
p Vis.v
