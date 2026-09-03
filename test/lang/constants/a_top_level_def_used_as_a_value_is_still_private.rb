# A top-level `def` is a PRIVATE instance method of Object. Writing it where a
# value is read does not change that -- `p(def m; end)` is the same definition,
# and only the `:m` it answers with is new. zeo registers the bare spelling at
# analyze time and reaches codegen's expression form for this one, which
# installs through `define_method` and so came out public.
p(def top; end)
p Object.private_instance_methods(false).include?(:top)
p Object.public_instance_methods(false).include?(:top)

# Callable through implicit self, and not with a receiver -- which is the whole
# observable consequence of the mark.
p top.nil?
begin
  self.top
rescue NoMethodError => e
  puts e.message
end
p Object.new.respond_to?(:top)
p Object.new.respond_to?(:top, true)

# A class body's default is PUBLIC, and the same value position must not borrow
# the top level's rule.
class K
  p(def in_class; end)
end
p K.public_instance_methods(false).include?(:in_class)
p K.new.in_class.nil?

# `def self.x` in a value position is a singleton method of `main`, not an
# Object instance method -- a different definee, so a different rule.
p(def self.on_main; end)
p Object.private_instance_methods(false).include?(:on_main)
p on_main.nil?
__END__
:top
true
false
true
false
true
:in_class
true
true
:on_main
false
true
