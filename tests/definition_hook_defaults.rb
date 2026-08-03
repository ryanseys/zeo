# Ruby tells a class what was just defined in it, through eight private hooks.
# This file covers only their DEFAULTS -- that they exist, that they are
# private, that `super` reaches them, and that the three mix-in primitives
# really do the mixing. What CALLS them is covered elsewhere.

p Module.private_instance_methods(false).grep(/features|extend_object|added|removed|undefined/).sort
p BasicObject.private_instance_methods(false).sort
p [Module.private_method_defined?(:prepend_features), Module.method_defined?(:prepend_features)]
p Module.instance_method(:const_added).arity

# Private means private: an explicit receiver is refused.
begin
  Comparable.prepend_features(Class.new)
rescue NoMethodError => e
  puts "explicit: #{e.message}"
end

# The defaults answer nil and take exactly one argument.
class Probe
  def self.reach(hook, arg) = send(hook, arg)
end
%i[method_added method_removed method_undefined const_added].each do |hook|
  puts "#{hook} -> #{Probe.reach(hook, :x).inspect}"
end
%i[singleton_method_added singleton_method_removed singleton_method_undefined].each do |hook|
  puts "#{hook} -> #{Object.new.send(hook, :x).inspect}"
end

# `prepend_features`/`append_features`/`extend_object` are the primitives that
# perform the mixin -- not notifications. Called directly, they mix in.
m = Module.new { def hi = "hi from M" }
k = Class.new
m.send(:prepend_features, k)
p [k.ancestors.include?(m), k.new.hi]

n = Module.new { def yo = "yo from N" }
k2 = Class.new
n.send(:append_features, k2)
p [k2.ancestors.include?(n), k2.new.yo]

o = Object.new
Module.new { def ext = "extended" }.send(:extend_object, o)
p o.ext

# An override reaches the default through `super`, which is the whole reason
# the defaults exist.
class Sub
  def self.method_added(name)
    puts "Sub saw #{name.inspect}, super -> #{super.inspect}"
  end
end
p Sub.send(:method_added, :manual)
