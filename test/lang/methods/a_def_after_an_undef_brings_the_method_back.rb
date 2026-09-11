# A `def` after the same body's `undef` of that name brings the method back
# as a PUBLIC row of the class, whether the body is in a required file or in
# the main one, on a builtin or on a user class: it leaves the private list,
# `send` reaches it, and a plain call answers the new body.
def report(label, mod, name)
  puts "#{label}: priv=#{mod.private_method_defined?(name)} pub=#{mod.public_method_defined?(name)} " \
       "privlist=#{mod.private_instance_methods(false).include?(name)} " \
       "publist=#{mod.public_instance_methods(false).include?(name)} " \
       "owner=#{mod.instance_method(name).owner}"
end
require_relative "a_def_after_an_undef_brings_the_method_back/unit"
report("unit method_added", Module, :method_added)
p Module.new.send(:method_added, :x)

class ::Module
  undef method_removed
  def method_removed(mid) = :mine
end
report("main method_removed", Module, :method_removed)
p Module.new.send(:method_removed, :x)

class String
  undef upcase
  def upcase = :mine
end
report("main upcase", String, :upcase)
p "a".upcase

class Foo
  def bar = 1
  undef bar
  def bar = 2
end
p Foo.new.bar
__END__
unit method_added: priv=false pub=true privlist=false publist=true owner=Module
:unit
main method_removed: priv=false pub=true privlist=false publist=true owner=Module
:mine
main upcase: priv=false pub=true privlist=false publist=true owner=String
:mine
2
