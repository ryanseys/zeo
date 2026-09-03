# `alias_method` names a method the class body itself just created. bundler's
# `Runtime` is the shape: a `define_method` wrapper called five times, then
# `alias_method :gems, :specs`. The source only exists once the body has run,
# so the alias has to be checked THERE and not before.
class Runtime
  def self.definition_method(meth)
    define_method(meth) { "answering #{meth}" }
  end
  private_class_method :definition_method

  definition_method :specs
  definition_method :requires

  alias_method :gems, :specs
end
r = Runtime.new
p r.specs, r.gems, r.requires
p Runtime.instance_methods(false).sort
p Runtime.method_defined?(:gems)
p r.method(:gems).call

# An alias of an ordinary `def` in the same body still works, and so does one
# of an inherited method.
class Base
  def greet = "hi"
end
class Sub < Base
  def shout = "HI"
  alias_method :yell, :shout
  alias_method :hello, :greet
end
p Sub.new.yell, Sub.new.hello
p Sub.instance_methods(false).sort

# ...and a reopen aliasing something the FIRST body defined.
class Reopened
  define_method(:original) { :first }
end
class Reopened
  alias_method :second, :original
end
p Reopened.new.second

# A genuinely missing source is still a NameError, raised where the body runs.
begin
  Class.new { alias_method :nope, :no_such_method_anywhere }
rescue NameError => e
  puts e.class
end
__END__
"answering specs"
"answering specs"
"answering requires"
[:gems, :requires, :specs]
true
"answering specs"
"HI"
"hi"
[:hello, :shout, :yell]
:first
NameError
