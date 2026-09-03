# The same six hooks as `definition_hooks_at_runtime.rb`, but for definitions
# written in a class body -- which zeo compiles into a method table rather than
# running, so the report has to be put back at the definition's position.
#
# Two rules do most of the work here. A `def self.x` (or one in `class << self`)
# reports through `singleton_method_added`, not `method_added`. And a hook
# INSTALLED after a definition never saw it, so only the definitions written
# below it report.

puts "== top-level def, on Object"
class Object
  def self.method_added(n) = puts("Object added #{n}")
end
def toplevel_one; end

puts "== every shape a class body can write"
class C
  # These two are written before the singleton hook below, so neither reports.
  def self.method_added(n) = puts("C added #{n}")
  def self.method_undefined(n) = puts("C undefined #{n}")
  # This one IS installed by the time it finishes, so it reports itself.
  def self.singleton_method_added(n) = puts("C s_added #{n}")

  def a; end
  puts "  (an ordinary statement between defs keeps its place)"
  def b; end
  def b; end # a redefinition reports again
  attr_accessor :acc # reader, then writer
  attr_reader :ro
  alias_method :c, :a
  alias d a
  def self.klass_method; end
  class << self
    def via_sclass; end
  end
end

puts "== a reopen reports too, and `undef` has its own hook"
class C
  def reopened; end
  undef_method :a
end

puts "== a nested class has no hook of its own, so Object's answers"
class Outer
  def self.method_added(n) = puts("Outer added #{n}")
  class Inner
    def inner_method; end
  end
  def outer_method; end
end

puts "== a subclass inherits the hook"
class SubC < C
  def sub_method; end
end

puts "== a bare `private` changes visibility, not whether the def reports"
class V
  def self.method_added(n) = puts("V added #{n}")
  private
  def hidden; end
  public
  def shown; end
end

puts "== bare `module_function` reports both halves"
module MF
  def self.method_added(n) = puts("MF added #{n}")
  def self.singleton_method_added(n) = puts("MF s_added #{n}")
  module_function
  def after_modfunc; end
end
# Both halves are the same definition, so both name the `def`'s own line.
p [MF.method(:after_modfunc).source_location, MF.instance_method(:after_modfunc).source_location]
__END__
== top-level def, on Object
Object added toplevel_one
== every shape a class body can write
C s_added singleton_method_added
C added a
  (an ordinary statement between defs keeps its place)
C added b
C added b
C added acc
C added acc=
C added ro
C added c
C added d
C s_added klass_method
C s_added via_sclass
== a reopen reports too, and `undef` has its own hook
C added reopened
C undefined a
== a nested class has no hook of its own, so Object's answers
Object added inner_method
Outer added outer_method
== a subclass inherits the hook
C added sub_method
== a bare `private` changes visibility, not whether the def reports
V added hidden
V added shown
== bare `module_function` reports both halves
MF s_added singleton_method_added
MF added after_modfunc
MF s_added after_modfunc
[["lang/classes/definition_hooks_when_compiled.rb", 72], ["lang/classes/definition_hooks_when_compiled.rb", 72]]
