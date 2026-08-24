# A method of a RUNTIME-minted class names its frame after that class.
#
# A `def` inside such a body is lifted to a top-level method and installed on
# the class afterwards, so the emitter bakes `Object#name` -- the only owner it
# can see. Ruby names the frame from the class's name AT RAISE TIME, and that
# is not a compile-time fact: the same anonymous class reports a bare method
# name while held in a local, and `K#m` once a constant assignment names it.
#
# So the installer hands the label to the next frame push. The handover is
# ONE-SHOT and replaces only the `Object#` fallback, and both narrowings are
# load-bearing -- see the last two sections.

def frames(label, n = 1)
  yield
rescue => e
  puts "#{label}:"
  puts e.backtrace.first(n).map { |l| "  " + l.sub(%r{\A.*?([^/]+):}, '\1:') }
end

# --- named by a constant assignment
K = Class.new do
  def m
    raise "k"
  end
end
frames("constant-named") { K.new.m }

# --- anonymous: ruby prints the bare method name, not an invented owner
anon = Class.new do
  def m
    raise "anon"
  end
end
frames("anonymous") { anon.new.m }

# --- a `class X < RuntimeValue` body, which takes the same lift
class Sub < K
  def n
    raise "sub"
  end
end
frames("subclass of a runtime class") { Sub.new.n }

# --- a module minted the same way
M = Module.new do
  def mm
    raise "mod"
  end
end
class UsesM
  include M
end
frames("runtime module method") { UsesM.new.mm }

# --- a nested runtime class under a runtime namespace
NS = Class.new
class NS::Inner < K
  def deep
    raise "deep"
  end
end
frames("nested under a runtime namespace") { NS::Inner.new.deep }

# --- THE ONE-SHOT MUST NOT LEAK. A top-level `def` called from inside a
# runtime class's method keeps its own `Object#` label; matching on the name
# alone would have renamed it after the calling class.
def helper
  raise "helper"
end
Caller = Class.new do
  def outer
    helper
  end
end
frames("top-level def called from a runtime method", 2) { Caller.new.outer }

# --- A GENUINE `define_method` BLOCK keeps its block label. The same install
# path carries both, so overriding unconditionally renamed every one of these.
Blocky = Class.new do
  define_method(:dm) { raise "dm" }
end
frames("define_method block") { Blocky.new.dm }

# --- an ordinary compiled class is untouched
class Plain
  def p1
    raise "plain"
  end
end
frames("ordinary compiled class") { Plain.new.p1 }
