# Differential probe: ARGUMENT VALIDATION.
#
# Feeds deliberately-wrong arguments to the reflection and collection
# surface and prints the exception class and message per row. Run through
# `tools/zeo-dev probe arguments`, which diffs the rows against ruby 4.0.6
# and lists only the ones that disagree.
#
# The point is to convert a long tail of one-off "zeo accepts what ruby
# refuses" findings into a finite list. Sixteen gap files were written one
# at a time before this existed.
#
# Add rows freely; a row that AGREES costs one line of output and becomes
# regression cover the moment someone breaks it.

ROWS = {}

def probe(name, &blk) = ROWS[name] = blk

# --- Names: ivars, constants, attributes, methods -------------------------
o = Object.new
probe("instance_variable_get(@@x)") { o.instance_variable_get("@@x") }
probe("instance_variable_get(x)") { o.instance_variable_get("x") }
probe("instance_variable_get(@1x)") { o.instance_variable_get("@1x") }
probe("instance_variable_get(@)") { o.instance_variable_get("@") }
probe("instance_variable_get(sym)") { o.instance_variable_get(:@ok) }
probe("instance_variable_set(bad)") { o.instance_variable_set("@@x", 1) }
probe("instance_variable_defined?(bad)") { o.instance_variable_defined?("x") }
probe("remove_instance_variable(bad)") { o.remove_instance_variable("x") }
probe("remove_instance_variable(absent)") { o.remove_instance_variable(:@nope) }

k = Class.new
probe("const_set(lower)") { k.const_set(:lower, 1) }
probe("const_set(path)") { k.const_set("A::B", 1) }
probe("const_set(empty)") { k.const_set("", 1) }
probe("const_set(digit)") { k.const_set("1A", 1) }
probe("const_get(lower)") { k.const_get(:lower) }
probe("const_get(absent)") { k.const_get(:Absent) }
probe("remove_const(absent)") { k.send(:remove_const, :Absent) }

probe("attr_accessor(1bad)") { Class.new { attr_accessor :"1bad" } }
probe("attr_accessor(ok then bad)") do
  c = Class.new { attr_accessor :ok, :"1bad" }
  c.instance_methods(false).sort.inspect
end
probe("attr_accessor(bang)") { Class.new { attr_accessor :"x!" } }
probe("attr_accessor(int)") { Class.new { attr_accessor 1 } }
probe("define_method(no body)") { Class.new { define_method(:x) } }
probe("define_method(int)") { Class.new { define_method(:x, 1) } }
probe("alias_method(absent)") { Class.new { alias_method :a, :nope } }
probe("instance_method(absent)") { Class.new.instance_method(:nope) }

# --- Modules and classes --------------------------------------------------
probe("include(Class)") { Class.new { include String } }
probe("include(Integer)") { Class.new { include 1 } }
probe("include(self)") do
  m = Module.new
  m.send(:include, m)
end
probe("include(frozen, bad)") { Class.new.freeze.send(:include, String) }
probe("include(frozen, good)") { Class.new.freeze.send(:include, Comparable) }
probe("include(mod, Class)") { Class.new { include Comparable, String } }
probe("prepend(Class)") { Class.new { prepend String } }
probe("extend(Class)") { Object.new.extend(String) }
probe("Class.new(Module)") { Class.new(Module.new) }
probe("Class.new(Class)") { Class.new(Class) }
probe("Class.new(Integer)") { Class.new(1) }
probe("Class.new(singleton)") { Class.new(Object.new.singleton_class) }
probe("Module.new(arg)") { Module.new(1) }

# --- Procs and callables --------------------------------------------------
pr = proc { |x| x }
probe("proc >> Integer") { pr >> 7 }
probe("proc << Integer") { pr << 7 }
probe("proc >> Symbol") { (pr >> :to_s).call(1) }
probe("proc >> proc") { (pr >> proc { |v| v }).call(1) }
probe("Method#>> Integer") { 1.method(:+) >> 7 }
probe("curry(bad)") { pr.curry(:x) }

# --- Collections ----------------------------------------------------------
probe("bsearch(String)") { [1, 2, 3].bsearch { "x" } }
probe("bsearch(Float)") { [1, 2, 3].bsearch { 0.0 }.inspect }
probe("bsearch(nil)") { [1, 2, 3].bsearch { nil }.inspect }
probe("bsearch(NaN)") { [1, 2, 3].bsearch { Float::NAN }.inspect }
probe("Array#fill(bad)") { [1].fill(1, :x) }
probe("Array#first(-1)") { [1].first(-1) }
probe("Array#take(-1)") { [1].take(-1) }
probe("Array#sample(-1)") { [1].sample(-1) }
probe("Array#[]=(bad)") { [1][:x] = 1 }
probe("Array#flatten(bad)") { [1].flatten(:x) }
probe("Hash#fetch(absent)") { {}.fetch(:x) }
probe("Hash#merge(Integer)") { {}.merge(1) }
probe("Hash#dig(bad)") { { a: 1 }.dig(:a, :b) }
probe("Range#new(incomparable)") { Range.new(1, "a") }
probe("Range#step(0)") { (1..3).step(0).to_a }
probe("Range#step(-1)") { (1..3).step(-1).to_a }
probe("String#*(-1)") { "a" * -1 }
probe("String#[]=(absent)") { (+"a")["z"] = "b" }
probe("Integer(bad)") { Integer("z") }
probe("Integer(nil)") { Integer(nil) }
probe("Float(bad)") { Float("z") }
probe("Array#pack(bad)") { [1].pack("Z-") }
probe("String#unpack(bad)") { "a".unpack("Z-") }

# --- Queues and threads ---------------------------------------------------
probe("Queue.new(arg)") { Thread::Queue.new(1) }
probe("SizedQueue.new(0)") { Thread::SizedQueue.new(0) }
probe("SizedQueue.new(-1)") { Thread::SizedQueue.new(-1) }
probe("Queue#pop(true) empty") { Thread::Queue.new.pop(true) }
probe("Queue#pop(true, timeout:)") { Thread::Queue.new.pop(true, timeout: 1) }

# --- Marshal --------------------------------------------------------------
probe("Marshal.dump(proc)") { Marshal.dump(proc {}) }
probe("Marshal.dump(IO)") { Marshal.dump($stdout) }
probe("Marshal.dump(singleton)") do
  s = Object.new
  def s.x = 1
  Marshal.dump(s)
end
probe("Marshal.dump(extended)") do
  s = Object.new
  s.extend(Comparable)
  Marshal.dump(s).bytesize.positive?
end

ROWS.each do |name, fn|
  r = begin
    v = fn.call
    "ok #{v.inspect}"
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts "#{name}\t#{r}"
end
