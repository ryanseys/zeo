# `class X < <expression>` builds the class at runtime, so its body runs as a
# block -- and a construct only the static class path can emit has no runtime
# spelling. Reaching codegen with one used to abort the compiler with an
# "unexpected top-level-only node in expression position" panic; a local write
# would silently assign the ENCLOSING scope's variable instead of opening its
# own. Both are rejected by name.
#
# A REOPEN (`Foo = Class.new; class Foo ... end`) has a static fallback for
# exactly these bodies, so it takes that instead of rejecting -- see
# `reopening_a_runtime_class_falls_back_rather_than_failing_to_compile`.
#
# A runtime-built class body EMITS as a block, which shares the enclosing
# local scope -- but ruby gives a class body its own. The body's names are
# renamed per body (`rename::isolate_runtime_class_locals`), so neither
# direction leaks: the outer `y` keeps its value, and the body's `y` is
# invisible outside. This used to be a clean compile-time rejection.

y = 99
class Foo < Struct.new(:a)
  y = 1
  define_method(:from_body) { y }
end
p y
p Foo.new(0).from_body
p defined?(y)
p binding.local_variables
__END__
99
1
"local-variable"
[:y]
