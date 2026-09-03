# Differential probe: ARGUMENT VALIDATION.
#
# Feeds deliberately-wrong arguments to the reflection and collection
# surface and prints the exception class and message per row. A GOLDEN: the
# `.expected` beside it is ruby 4.0.6's own answers, so every row is a
# differential assertion and a divergence fails the suite.
#
# The point is to convert a long tail of one-off "zeo accepts what ruby
# refuses" findings into a finite list. Sixteen gap files were written one
# at a time before this existed.
#
# Add rows freely, then re-bless: a row that AGREES costs one line of output
# and becomes regression cover the moment someone breaks it.

ROWS = {}

def probe(name, &blk) = ROWS[name] = blk

# Rows zeo does not answer yet, each naming the gap that tracks it. A carved
# row is left OUT of the output rather than recorded wrong -- one row cannot
# be allowed to hold the other 74 out of the suite. When a gap is promoted,
# delete its entry here and re-bless; the row comes back.
SKIP = {
  "Module.new(arg)" => "tests/gaps/module_new_refuses_an_argument.rb",
}.freeze

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
  next if SKIP.key?(name)

  r = begin
    v = fn.call
    "ok #{v.inspect}"
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts "#{name}\t#{r}"
end
__END__
instance_variable_get(@@x)	NameError: '@@x' is not allowed as an instance variable name
instance_variable_get(x)	NameError: 'x' is not allowed as an instance variable name
instance_variable_get(@1x)	NameError: '@1x' is not allowed as an instance variable name
instance_variable_get(@)	NameError: '@' is not allowed as an instance variable name
instance_variable_get(sym)	ok nil
instance_variable_set(bad)	NameError: '@@x' is not allowed as an instance variable name
instance_variable_defined?(bad)	NameError: 'x' is not allowed as an instance variable name
remove_instance_variable(bad)	NameError: 'x' is not allowed as an instance variable name
remove_instance_variable(absent)	NameError: instance variable @nope not defined
const_set(lower)	NameError: wrong constant name lower
const_set(path)	NameError: wrong constant name A::B
const_set(empty)	NameError: wrong constant name 
const_set(digit)	NameError: wrong constant name 1A
const_get(lower)	NameError: wrong constant name lower
const_get(absent)	NameError: uninitialized constant #<Class:0xADDR>::Absent
remove_const(absent)	NameError: constant #<Class:0xADDR>::Absent not defined
attr_accessor(1bad)	NameError: invalid attribute name '1bad'
attr_accessor(ok then bad)	NameError: invalid attribute name '1bad'
attr_accessor(bang)	NameError: invalid attribute name 'x!'
attr_accessor(int)	TypeError: 1 is not a symbol nor a string
define_method(no body)	ArgumentError: tried to create Proc object without a block
define_method(int)	TypeError: wrong argument type Integer (expected Proc/Method/UnboundMethod)
alias_method(absent)	NameError: undefined method 'nope' for class '#<Class:0xADDR>'
instance_method(absent)	NameError: undefined method 'nope' for class '#<Class:0xADDR>'
include(Class)	TypeError: wrong argument type Class (expected Module)
include(Integer)	TypeError: wrong argument type Integer (expected Module)
include(self)	ArgumentError: cyclic include detected
include(frozen, bad)	TypeError: wrong argument type Class (expected Module)
include(frozen, good)	FrozenError: can't modify frozen Class: #<Class:0xADDR>
include(mod, Class)	TypeError: wrong argument type Class (expected Module)
prepend(Class)	TypeError: wrong argument type Class (expected Module)
extend(Class)	TypeError: wrong argument type Class (expected Module)
Class.new(Module)	TypeError: superclass must be an instance of Class (given an instance of Module)
Class.new(Class)	TypeError: can't make subclass of Class
Class.new(Integer)	TypeError: superclass must be an instance of Class (given an instance of Integer)
Class.new(singleton)	TypeError: can't make subclass of singleton class
proc >> Integer	TypeError: callable object is expected
proc << Integer	TypeError: callable object is expected
proc >> Symbol	TypeError: callable object is expected
proc >> proc	ok 1
Method#>> Integer	TypeError: callable object is expected
curry(bad)	TypeError: no implicit conversion of Symbol into Integer
bsearch(String)	TypeError: wrong argument type String (must be numeric, true, false or nil)
bsearch(Float)	ok "2"
bsearch(nil)	ok "nil"
bsearch(NaN)	ArgumentError: comparison of Float with 0 failed
Array#fill(bad)	TypeError: no implicit conversion of Symbol into Integer
Array#first(-1)	ArgumentError: negative array size
Array#take(-1)	ArgumentError: attempt to take negative size
Array#sample(-1)	ArgumentError: negative sample number
Array#[]=(bad)	TypeError: no implicit conversion of Symbol into Integer
Array#flatten(bad)	TypeError: no implicit conversion of Symbol into Integer
Hash#fetch(absent)	KeyError: key not found: :x
Hash#merge(Integer)	TypeError: no implicit conversion of Integer into Hash
Hash#dig(bad)	TypeError: Integer does not have #dig method
Range#new(incomparable)	ArgumentError: bad value for range
Range#step(0)	ArgumentError: step can't be 0
Range#step(-1)	ok []
String#*(-1)	ArgumentError: negative argument
String#[]=(absent)	IndexError: string not matched
Integer(bad)	ArgumentError: invalid value for Integer(): "z"
Integer(nil)	TypeError: can't convert nil into Integer
Float(bad)	ArgumentError: invalid value for Float(): "z"
Array#pack(bad)	TypeError: no implicit conversion of Integer into String
String#unpack(bad)	ArgumentError: unknown unpack directive '-' in 'Z-'
Queue.new(arg)	TypeError: can't convert Integer into Array
SizedQueue.new(0)	ArgumentError: queue size must be positive
SizedQueue.new(-1)	ArgumentError: queue size must be positive
Queue#pop(true) empty	ThreadError: queue empty
Queue#pop(true, timeout:)	ArgumentError: can't set a timeout if non_block is enabled
Marshal.dump(proc)	TypeError: no _dump_data is defined for class Proc
Marshal.dump(IO)	TypeError: can't dump IO
Marshal.dump(singleton)	TypeError: singleton can't be dumped
Marshal.dump(extended)	ok true
