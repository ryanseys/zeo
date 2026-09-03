# `::` takes a class or module on its left, and that left side is an ordinary
# EXPRESSION -- `self`, a parameter, a method call. Only the name after `::` has
# to be spelled at compile time. So the whole search happens at run time: the
# scope operator's own search (a constant `Object` owns stays out of reach, and
# a `private_constant` is hidden), never `const_get`'s, which reaches through
# both and which a class may override.
#
# activerecord's `ColumnDefinition = Struct.new(...) do self::OPTION_NAMES =
# [...] end` is the shape, and orm_adapter writes the same thing inside an
# `ActiveSupport.on_load` block, where nothing NAMES the class at all.

class Named
  self::FOO = 1
end
p Named::FOO

# An anonymous class has no name to write, which is the point of the form.
anon = Class.new
anon::NAME = :set
p anon.const_get(:NAME)
p anon::NAME

# The scope is evaluated FIRST, then the value, and the "not a module" check
# comes after BOTH -- `1::X = (puts ...; 3)` prints before it raises.
def scope_of(m)
  puts "scope"
  m
end

holder = Module.new
scope_of(holder)::VALUE = (puts "value"; 42)
p holder::VALUE

begin
  1::WHAT = (puts "rhs still ran"; 2)
rescue TypeError => e
  p e.message
end

begin
  Object.new::ANY
rescue TypeError => e
  p e.message.sub(/0x\h+/, "0xXXXX")
end

# A plain `obj::NAME = v` inside a method is the `dynamic constant assignment`
# SyntaxError -- but `||=` is not, which is the loophole act_as_attribute's
# `self::AVAILABLE_ATTRIBUTES ||= []` is built on. Never assigned reads as nil
# and the write defines it, exactly as a bare `CONST ||= v` does.
module Registry; end

def stock(m)
  m::LIST ||= []
  m::LIST << :entry
  m::COUNT ||= 0
end

stock(Registry)
stock(Registry)
p Registry::LIST
p Registry::COUNT

# `+=` and `&&=` keep the RAISING read, again matching the bare form.
module Counter
  START = 10
end

def bump(m)
  m::START += 1
end
p bump(Counter)
p Counter::START

def guard(m)
  m::MISSING &&= :never
rescue NameError => e
  e.message
end
p guard(Counter)

# The scope expression is evaluated ONCE per assignment, not once per half.
$calls = 0
def counted(m)
  $calls += 1
  m
end

module Once; end
counted(Once)::SEEN ||= :first
counted(Once)::SEEN ||= :second
p [$calls, Once::SEEN]

# Reading through a value works wherever a constant does -- as a rescue class,
# as a raise class, as a superclass check.
Errors = Module.new
Errors.const_set(:Denied, Class.new(StandardError))

def attempt(mod)
  raise mod::Denied, "no"
rescue mod::Denied => e
  e.message
end
p attempt(Errors)

# `defined?` answers "constant" or nil, and swallows the TypeError a non-module
# scope would otherwise raise.
def defined_pair(x)
  [defined?(x::Denied), defined?(x::Absent)]
end
p defined_pair(Errors)
p defined_pair(7)

# The scope operator does NOT reach a top-level constant through the scope's
# ancestry, even though `const_get` does.
TOP_LEVEL = :visible

def through(m)
  m::TOP_LEVEL
rescue NameError => e
  e.message.sub(/0x\h+/, "0xXXXX")
end

plain = Module.new
p through(plain)
p plain.const_get(:TOP_LEVEL)

# ... nor a `private_constant`.
class Sealed
  HIDDEN = :secret
  private_constant :HIDDEN
end

def peek(k)
  k::HIDDEN
rescue NameError => e
  e.message
end
p peek(Sealed)
p defined?(Sealed::HIDDEN)

# An ancestor's constant DOES answer, since the search walks the chain.
module Base
  SHARED = :inherited
end

class Derived
  include Base
end

def shared(k)
  k::SHARED
end
p shared(Derived)

# `self` in a class body is the class, so `self::NAME` and a bare `NAME` name
# the same constant.
class Both
  self::ONE = 1
  TWO = 2
  p [self::ONE, self::TWO, ONE, TWO]
end
__END__
1
:set
:set
scope
value
42
rhs still ran
"1 is not a class/module"
"#<Object:0xXXXX> is not a class/module"
[:entry, :entry]
0
11
11
"uninitialized constant Counter::MISSING"
[2, :first]
"no"
["constant", nil]
[nil, nil]
"uninitialized constant #<Module:0xXXXX>::TOP_LEVEL"
:visible
"private constant Sealed::HIDDEN referenced"
nil
:inherited
[1, 2, 1, 2]
#@ stderr
lang/constants/a_constant_path_may_be_rooted_at_a_value.rb:69: warning: already initialized constant Counter::START
lang/constants/a_constant_path_may_be_rooted_at_a_value.rb:65: warning: previous definition of START was here
