# The reflection surface validates the NAMES it is given, through one
# shared set of predicates rather than sixteen separate remembered checks.
#
# The five promoted gaps beside this file each cover one row. This covers
# the RULES, which is where the next miss would come from -- and three of
# them are surprising enough to be worth pinning:
#
#   * an identifier takes no trailing `?`/`!`/`=` (`attr_accessor :x?` is
#     refused), while non-ASCII letters ARE identifier characters;
#   * a constant is a SINGLE segment for `const_set` and a PATH for
#     `const_get`, so `const_set("A::B")` is a wrong-name error rather than
#     a nested write;
#   * the NAME is checked before FROZEN, and per ARGUMENT rather than once
#     up front, both of which are observable.

def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message.sub(/0x[0-9a-f]+/, "0xN")}"
end

# --- Instance-variable names ----------------------------------------------
o = Object.new
show { o.instance_variable_get("@@bad") }
show { o.instance_variable_get("@1x") }
show { o.instance_variable_get("@") }
show { o.instance_variable_get("x") }
show { o.instance_variable_set("@@bad", 1) }
show { o.instance_variable_defined?("@@bad") }
# Valid, including a non-ASCII name.
show { o.instance_variable_set(:@ok, 1) }
show { o.instance_variable_get(:@ok) }
show { o.instance_variable_set(:@日本, 2) }
show { o.instance_variable_get(:@日本) }
show { o.instance_variables }

# --- Constant names -------------------------------------------------------
show { Module.new.const_set(:lower, 1) }
show { Module.new.const_set("A::B", 1) }
show { Module.new.const_set("", 1) }
show { Module.new.const_set("1A", 1) }
show { Module.new.const_set(:Ok, 1) }
show { Module.new.const_get(:lower) }
show { Module.new.send(:remove_const, :Nope) }
# `const_get` takes a PATH where `const_set` takes a segment.
show { Object.const_get("Comparable") }

# --- Attribute names ------------------------------------------------------
show { Class.new { attr_accessor :"1bad" } }
show { Class.new { attr_accessor :"x?" } }
show { Class.new { attr_accessor :"x!" } }
show { Class.new { attr_accessor 1 } }
# The name is checked per ARGUMENT: `:ok` is defined, and then it raises.
show do
  c = Class.new
  begin
    c.send(:attr_accessor, :ok, :"1bad")
  rescue NameError
    nil
  end
  c.instance_methods(false).sort
end
# A frozen class given a BAD name says NameError, not FrozenError.
show { Class.new.freeze.send(:attr_accessor, :"1bad") }
show { Class.new.freeze.send(:attr_accessor, :ok) }

# --- Module arguments -----------------------------------------------------
show { Class.new { include String } }
show { Class.new { include 1 } }
show { Class.new { prepend String } }
show { Object.new.extend(String) }
show do
  m = Module.new
  m.send(:include, m)
end
show do
  m = Module.new
  n = Module.new { include m }
  m.send(:include, n)
end
# Every argument is validated BEFORE any is spliced.
show do
  c = Class.new
  begin
    c.send(:include, Comparable, String)
  rescue TypeError
    nil
  end
  c.ancestors.include?(Comparable)
end
# Frozen sits between the type check and the cycle check.
show { Class.new.freeze.send(:include, String) }
show { Class.new.freeze.send(:include, Comparable) }

# --- Superclasses ---------------------------------------------------------
show { Class.new(Comparable) }
show { Class.new(1) }
show { Class.new(Class) }
show { Class.new(Object.new.singleton_class) }
# LEGAL, and both used to be conflated with the refusals: `Module` is a
# Class, it is just not a module instance.
show { Class.new(Module).ancestors.include?(Module) }
show { Class.new(BasicObject).ancestors.include?(BasicObject) }
show { Class.new(String).new("hi") }
__END__
NameError: '@@bad' is not allowed as an instance variable name
NameError: '@1x' is not allowed as an instance variable name
NameError: '@' is not allowed as an instance variable name
NameError: 'x' is not allowed as an instance variable name
NameError: '@@bad' is not allowed as an instance variable name
NameError: '@@bad' is not allowed as an instance variable name
1
1
2
2
[:@ok, :@日本]
NameError: wrong constant name lower
NameError: wrong constant name A::B
NameError: wrong constant name 
NameError: wrong constant name 1A
1
NameError: wrong constant name lower
NameError: constant #<Module:0xN>::Nope not defined
Comparable
NameError: invalid attribute name '1bad'
NameError: invalid attribute name 'x?'
NameError: invalid attribute name 'x!'
TypeError: 1 is not a symbol nor a string
[:ok, :ok=]
NameError: invalid attribute name '1bad'
FrozenError: can't modify frozen Class: #<Class:0xN>
TypeError: wrong argument type Class (expected Module)
TypeError: wrong argument type Integer (expected Module)
TypeError: wrong argument type Class (expected Module)
TypeError: wrong argument type Class (expected Module)
ArgumentError: cyclic include detected
ArgumentError: cyclic include detected
false
TypeError: wrong argument type Class (expected Module)
FrozenError: can't modify frozen Class: #<Class:0xN>
TypeError: superclass must be an instance of Class (given an instance of Module)
TypeError: superclass must be an instance of Class (given an instance of Integer)
TypeError: can't make subclass of Class
TypeError: can't make subclass of singleton class
true
true
"hi"
