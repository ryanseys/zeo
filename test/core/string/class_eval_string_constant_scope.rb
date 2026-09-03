# The STRING forms of `class_eval`/`module_eval`/`instance_eval` evaluate with
# the receiver as the constant lookup scope. zeo evaluates them with the
# caller's scope, so a constant owned by the receiver is not found.
#
#     Outer::Host.class_eval("HOST_C")   ruby :host   zeo NameError
#
# The BLOCK form is the opposite and zeo gets it right: a block keeps the
# lexical scope where it was written, which is why `class_eval { VAL }` sees
# the caller's constants and `class_eval("VAL")` does not. That contrast is the
# documented reason both forms exist, and a library that builds code as a
# string -- the classic `class_eval <<~RUBY` accessor generator -- depends on
# the string form seeing the receiver.
#
# The NameError message shows the same missing scope: ruby names the constant
# it looked for as `Outer::Host::VAL`, zeo as a bare `VAL`.

module Outer
  VAL = :outer_val
  class Host
    HOST_C = :host
  end
end

p Outer::Host.class_eval("HOST_C")

# `instance_eval`'s scope is the SINGLETON class, so an instance-level constant
# is correctly absent there -- but the message names the scope it searched, and
# that is the same information the case above is missing.
begin
  Outer::Host.instance_eval("HOST_C")
rescue NameError => e
  puts "#{e.class}: #{e.message}"
end

begin
  Outer::Host.class_eval("VAL")
rescue NameError => e
  puts "#{e.class}: #{e.message}"
end

# The block form keeps the caller's lexical scope -- already correct.
p Outer::Host.class_eval { Outer::VAL }
p Outer::Host.class_eval { const_defined?(:HOST_C) }

# A string that DEFINES a method lands on the receiver either way.
Outer::Host.class_eval("def from_string = HOST_C")
p Outer::Host.new.from_string
__END__
:host
NameError: uninitialized constant #<Class:Outer::Host>::HOST_C
NameError: uninitialized constant Outer::Host::VAL
:outer_val
true
:host
