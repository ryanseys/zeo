# Ruby runs no visibility check against a literal `self` receiver (private has
# been reachable through one since 2.7), nor against the receiver it
# synthesizes for a call written with none. Every other explicit receiver
# still meets the barrier.

class Acct
  def initialize(n)
    @n = n
  end

  def via_self = self.secret
  def via_nothing = secret
  def via_safe_nav = self&.secret
  def via_send = self.send(:secret)
  def with_args = self.tagged("x", 2)
  def with_kwargs = self.tagged("x", 2, sep: "-")
  def with_splat = self.tagged(*["x", 2])
  def with_block = self.each_twice { |i| i }
  def richer?(other) = self.balance > other.balance

  protected

  def balance = @n

  private

  def secret = "secret #{@n}"

  def tagged(a, b, sep: ":")
    [a, b].join(sep)
  end

  def each_twice
    [yield(1), yield(2)]
  end
end

a = Acct.new(1)
p a.via_self
p a.via_nothing
p a.via_safe_nav
p a.via_send
p a.with_args
p a.with_kwargs
p a.with_splat
p a.with_block
p a.richer?(Acct.new(0))

# An ordinary explicit receiver still raises -- for both verbs.
begin
  a.secret
rescue NoMethodError => e
  puts e.message
end
begin
  a.balance
rescue NoMethodError => e
  puts e.message
end

# A class body's `self` is the class, and the rule holds there too.
class Cfg
  class << self
    def build = self.prepare
    def build_splat = self.prepare(*[])

    private

    def prepare(*) = :prepared
  end
end
p Cfg.build
p Cfg.build_splat
begin
  Cfg.prepare
rescue NoMethodError => e
  puts e.message
end

# `class << self` writes its visibility verbs with no receiver at all: zeo
# synthesizes one, and that synthetic receiver must not raise the barrier.
# (The verbs' RUNTIME form -- `private(*names)` -- is a separate question
# about which table a singleton body writes; see the singleton-visibility
# golden.)
class Vis
  class << self
    def a = :a
    def b = :b
    def c = :c

    private :a
  end
end
p Vis.b
p Vis.c
begin
  Vis.a
rescue NoMethodError => e
  puts e.message
end

# A module body, same two shapes.
module Mod
  def self.run = self.helper

  class << self
    private

    def helper = :helped
  end
end
p Mod.run
