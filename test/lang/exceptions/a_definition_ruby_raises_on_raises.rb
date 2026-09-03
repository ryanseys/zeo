# A `class Foo` after `module Foo`, and a reopen that names a different
# superclass, are both TypeError in ruby -- a real exception at the definition,
# not a broken program. zeo used to REFUSE these at compile time whenever no
# `rescue` was lexically visible inside the definition's own statement, on the
# reasoning that an uncatchable raise aborts and a compile error naming the
# same problem is the better version of aborting.
#
# Two things were wrong with that. The lexical scan cannot see the handler a
# CALLER wraps the `require` in, which catches these perfectly well. And four
# of the five registration sites never consulted the scan at all, so the shape
# below -- a top-level kind collision, which is every `hola_*` tutorial gem on
# rubygems, and a whole family of generated SDKs that declare one class name
# under two superclasses -- could not be compiled either way.
#
# Both halves of the message are ruby's, including the second line, whose
# position is the one stamped when the CONSTANT was created.
module Foo
  X = 1
end

begin
  class Foo
    def never = "unreachable"
  end
rescue TypeError => e
  puts e.message
end

# The definition that raised changed nothing: ruby leaves the module exactly
# as it was.
p Foo::X
p Foo.instance_of?(Module)
p Foo.respond_to?(:never)

class Base; end
class Other; end
class Sub < Base; end

begin
  class Sub < Other; end
rescue TypeError => e
  puts e.message
end

p Sub.superclass
__END__
Foo is not a class
lang/exceptions/a_definition_ruby_raises_on_raises.rb:17: previous definition of Foo was here
1
true
false
superclass mismatch for class Sub
Base
