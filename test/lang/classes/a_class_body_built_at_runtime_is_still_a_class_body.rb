# `class Adapter < parent` with a LOCAL superclass has no compile-time class to
# lay out, so zeo builds it with `Class.new(parent) { ... }`. A block is not a
# class body in two ways ruby cares about, and both used to leak:
#
#   * a block SHARES the enclosing local scope, where a class body has its own;
#   * a block opens no cref, so `@@v` in one raised "class variable access from
#     toplevel".
#
# sidekiq's ActiveJob adapter is built exactly this way and hits both at once.

class AbstractAdapter
  def kind = :abstract
end

parent = AbstractAdapter
callback = :OUTER_UNTOUCHED

class Adapter < parent
  @@stopping = false

  # A body local, read from two nested blocks. Sharing the enclosing scope
  # would have clobbered the `callback` above.
  callback = -> { @@stopping = true }

  [1].each { |_| [2].each { |_| callback.call } }

  def self.stopping? = @@stopping

  def self.stop! = @@stopping = true
end

p Adapter.stopping?
p callback
p [Adapter.new.kind, Adapter.superclass]

# The body's local never reached the enclosing scope.
p defined?(callback)
p binding.local_variables.sort

# The class variable is real storage on the built class, reachable from the
# methods it defined.
class Adapter
  def self.reset! = @@stopping = false
end
Adapter.reset!
p Adapter.stopping?
Adapter.stop!
p [Adapter.stopping?, Adapter.class_variable_get(:@@stopping)]

# ---------------------------------------------------------------------------
# `rescue => target` takes any assignable target, not just a local.
# activesupport writes `rescue => @setup_exception`.

class Worker
  attr_reader :err

  def run
    begin
      raise ArgumentError, "boom"
    rescue => @err
    end
    @err.message
  end
end

w = Worker.new
p [w.run, w.err.class]

# ... including through a splatted exception list, which is rspec-rails' shape.
class Matcher
  attr_reader :rescued

  def match_unless_raises(*exceptions)
    exceptions.unshift(Exception) if exceptions.empty?
    begin
      yield
      true
    rescue *exceptions => @rescued
      false
    end
  end
end

m = Matcher.new
p [m.match_unless_raises(TypeError) { raise TypeError, "t" }, m.rescued.message]
p [m.match_unless_raises(TypeError) { :fine }, m.rescued.message]

# A global and a class variable work the same way.
begin
  raise "to a global"
rescue => $caught
end
p $caught.message

class Holder
  def self.grab
    begin
      raise "to a cvar"
    rescue => @@last
    end
    @@last.message
  end
end
p Holder.grab

# The target is assigned BEFORE the clause's own body runs.
def order
  raise "x"
rescue => @seen
  @seen.message.upcase
end
p order

# ---------------------------------------------------------------------------
# `define_singleton_method(:name, &callable)` hands over a proc the caller
# already holds -- not a literal block. ddtrace, mcp and datasource all write
# it that way.

module Timer
  class << self
    def now_provider=(block)
      define_singleton_method(:now, &block)
    end
  end
end

Timer.now_provider = -> { 42 }
p Timer.now

o = Object.new
blk = proc { :from_proc }
o.define_singleton_method(:tag, &blk)
p [o.tag, o.singleton_methods]

# A literal block still takes the compile-time path.
o2 = Object.new
o2.define_singleton_method(:lit) { :from_literal }
p o2.lit

# The captured binding travels with the proc.
def make(value)
  proc { value }
end
o3 = Object.new
o3.define_singleton_method(:held, &make(:captured))
p o3.held
__END__
true
:OUTER_UNTOUCHED
[:abstract, AbstractAdapter]
"local-variable"
[:blk, :callback, :m, :o, :o2, :o3, :parent, :w]
false
[true, true]
["boom", ArgumentError]
[false, "t"]
[true, "t"]
"to a global"
"to a cvar"
"X"
42
[:from_proc, [:tag]]
:from_literal
:captured
