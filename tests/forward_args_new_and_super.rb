# `...` argument forwarding in the two positions that bypassed the shared
# call-argument lowering.
#
# `def m(...)` and a plain `n(...)` already worked -- the parameter desugars to
# `*__fwd_rest, **__fwd_kw, &__fwd_blk` and the call site re-expands the three.
# Two callers never reached that code:
#
#   Klass.new(...)  -- the STATIC `new` interception decided the call had no
#                      dynamic arguments, because it looked for a splat or a
#                      `**`, and a `...` is neither in prism's spelling.
#   super(...)      -- lowered its arguments with a hand-rolled loop that had
#                      no forwarding case at all.
#
# nokogiri is the reason this matters: `Reader.new(...)` in nokogiri/xml.rb is
# the first, and its dependents (xpath, loofah, capybara, rails-dom-testing,
# roo, savon, sanitize) inherit the failure without containing a `...`
# themselves. io-stream and strong_migrations are the second.

class Base
  def initialize(a, b = 2, *rest, key: "k", **opts, &blk)
    @seen = [a, b, rest, key, opts, blk ? blk.call : nil]
  end

  attr_reader :seen
end

# `super(...)`: every kind of argument crosses in one go.
class Child < Base
  def initialize(...)
    super(...)
  end
end

p Child.new(1).seen
p Child.new(1, 9, 10, 11, key: "z", extra: true).seen
p Child.new(1) { "block ran" }.seen

# A leading required parameter before the `...`, which is the shape
# io-stream's `def initialize(io, ...)` uses.
class Tagged < Base
  def initialize(tag, ...)
    @tag = tag
    super(...)
  end

  attr_reader :tag
end

t = Tagged.new("t", 5, key: "q")
p [t.tag, t.seen]

# `Klass.new(...)` through the static interception, receiverless and qualified.
module Outer
  class Made
    def initialize(*args, **opts)
      @args = args
      @opts = opts
    end

    attr_reader :args, :opts
  end

  def self.build(...)
    Made.new(...)
  end
end

m = Outer.build(1, 2, flag: true)
p [m.args, m.opts]

class Wrapper
  def self.make(...)
    new(...)
  end

  def initialize(*a)
    @a = a
  end

  attr_reader :a
end

p Wrapper.make(7, 8).a

# `...` reaching an ordinary send and a block-taking method, so the block half
# of the forwarding is exercised through both.
def collect(...)
  [1, 2, 3].map(...)
end
p collect { |x| x * 10 }
