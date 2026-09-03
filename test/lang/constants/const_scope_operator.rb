# `Object` is an ancestor of every class, so an ancestry walk that reads its
# constant table makes every top-level constant answer as every class's own:
# `SystemCallError::Errno` found the top-level `Errno` module, and a `FLAGS` a
# `Struct.new` block left at top level would answer for `Line::FLAGS`.
#
# Ruby draws the line with one flag (`variable.c`'s `exclude`). The scope
# OPERATOR rejects a hit `Object` owns; `const_get`/`const_defined?` accept it.
# Everything below is that one difference, seen from both sides.

module M; end
class K; end

def try(label)
  v = yield
  puts "#{label} => #{v.inspect}"
rescue => e
  puts "#{label} !! #{e.class}: #{e.message}"
end

TOP = 4

# `::` cannot see a constant `Object` owns -- neither a top-level class nor a
# top-level value.
try("K::Errno") { K::Errno }
try("SystemCallError::Errno") { SystemCallError::Errno }
try("BasicObject::Errno") { BasicObject::Errno }
try("M::Errno") { M::Errno }
try("K::TOP") { K::TOP }
try("K::String") { K::String }

# ...but a receiver that IS `Object` reads them, since the rule excludes the
# ancestor rather than the receiver.
try("Object::Errno") { Object::Errno }
try("Object::TOP") { Object::TOP }
try("::TOP") { ::TOP }

# `const_get`/`const_defined?` ask the other question, and get the other answer.
try("K.const_get(:Errno)") { K.const_get(:Errno) }
try("K.const_get(:TOP)") { K.const_get(:TOP) }
try("M.const_get(:Errno)") { M.const_get(:Errno) }
try("K.const_defined?(:TOP)") { K.const_defined?(:TOP) }
try("K.const_defined?(:TOP, false)") { K.const_defined?(:TOP, false) }
try("M.const_defined?(:TOP)") { M.const_defined?(:TOP) }

# `defined?` gates the scope operator, so it answers the operator's way.
try("defined? K::TOP") { defined?(K::TOP) }
try("defined? K::Errno") { defined?(K::Errno) }
try("defined? Object::TOP") { defined?(Object::TOP) }

# Only the HEAD of a `const_get` path gets `const_get`'s reach. Every segment
# past it is a scope operator.
try("Object.const_get('K::TOP')") { Object.const_get("K::TOP") }
try("Object.const_get('K')") { Object.const_get("K") }

# A REAL ancestor still answers through `::` -- it is `Object` that is out of
# reach, not inheritance.
class Base
  INSIDE = 1
end
class Deriv < Base; end
module Mixin
  VIA = 2
end
class WithMix
  include Mixin
end
try("Deriv::INSIDE") { Deriv::INSIDE }
try("WithMix::VIA") { WithMix::VIA }
try("File::SEEK_SET") { File::SEEK_SET }
try("Deriv.const_get(:INSIDE, false)") { Deriv.const_get(:INSIDE, false) }

# The NameError names the missing leaf and the SCOPE it was asked of, not
# whatever ancestor the lookup ended on.
begin
  K::TOP
rescue NameError => e
  p [e.name, e.receiver, e.message]
end
begin
  K::Nope
rescue NameError => e
  p [e.name, e.receiver, e.message]
end

# A scoped WRITE targets the scope, and does not disturb the top-level name it
# cannot read.
K::TOP = 5
p [TOP, K::TOP, K.const_get(:TOP), K.constants]
__END__
K::Errno !! NameError: uninitialized constant K::Errno
SystemCallError::Errno !! NameError: uninitialized constant SystemCallError::Errno
BasicObject::Errno !! NameError: uninitialized constant BasicObject::Errno
M::Errno !! NameError: uninitialized constant M::Errno
K::TOP !! NameError: uninitialized constant K::TOP
K::String !! NameError: uninitialized constant K::String
Object::Errno => Errno
Object::TOP => 4
::TOP => 4
K.const_get(:Errno) => Errno
K.const_get(:TOP) => 4
M.const_get(:Errno) => Errno
K.const_defined?(:TOP) => true
K.const_defined?(:TOP, false) => false
M.const_defined?(:TOP) => true
defined? K::TOP => nil
defined? K::Errno => nil
defined? Object::TOP => "constant"
Object.const_get('K::TOP') !! NameError: uninitialized constant K::TOP
Object.const_get('K') => K
Deriv::INSIDE => 1
WithMix::VIA => 2
File::SEEK_SET => 0
Deriv.const_get(:INSIDE, false) !! NameError: uninitialized constant Deriv::INSIDE
[:TOP, K, "uninitialized constant K::TOP"]
[:Nope, K, "uninitialized constant K::Nope"]
[4, 5, 5, [:TOP]]
