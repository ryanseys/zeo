# `CONST.merge(opts)` where CONST is a Symbol-keyed hash and `opts`
# is a method parameter with an empty-hash default (`def m(opts = {})`).
# CRuby folds trailing keyword arguments into a Symbol-keyed positional
# Hash when the callee declares no keyword params, so the kwargs reach
# `opts` and `merge` dispatches sym-keyed. The empty default must also
# merge cleanly (returning the receiver's defaults).
#
# Output is per-key (not hash#inspect) so it is independent of the
# reference Ruby's hash formatting (3.2 `{:k=>v}` vs 3.4+ `{k: v}`).

DEFAULTS = { timeout: 30, host: "localhost" }

def configure(opts = {})
  DEFAULTS.merge(opts)
end

# Keyword-call form: kwargs collapse into the positional hash.
c1 = configure(timeout: 5)
puts c1[:timeout]                 # 5  (override applied)
puts c1[:host]                    # localhost (default kept)

# No-arg form: the empty `{}` default merges to just the defaults.
c2 = configure
puts c2[:timeout]                 # 30
puts c2[:host]                    # localhost

# The same idiom inside a constructor, the shape the spinelgems
# harvest cluster (rack-health, stream-chat-ruby, ...) uses.
class Client
  def initialize(opts = {})
    @config = DEFAULTS.merge(opts)
  end
  def config; @config; end
end

c3 = Client.new(host: "example.com", timeout: 5).config
puts c3[:timeout]                 # 5
puts c3[:host]                    # example.com

c4 = Client.new.config
puts c4[:timeout]                 # 30
puts c4[:host]                    # localhost

# A method that genuinely declares keyword params must NOT collapse:
# named kwargs bind to their own slots, and `**h` double-splat forwards.
def greet(name:, greeting: "hi")
  "#{greeting}, #{name}"
end
puts greet(name: "Ada")           # hi, Ada
opts = { name: "Bob", greeting: "yo" }
puts greet(**opts)                # yo, Bob
