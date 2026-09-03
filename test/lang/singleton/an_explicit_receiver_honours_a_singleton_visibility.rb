# The explicit-receiver barrier does not read a per-object singleton's
# visibility, so `obj.hidden` runs a method CRuby refuses.
#
# The barrier asks `instance_method_visibility(recv.class_id(), name)`. For an
# object with its own rows that names the ORDINARY class, which knows nothing
# about them -- they are keyed by identity, not by a class id. `public_send`
# reaches the same question through `send_value_public_in`.
#
# Both spellings of the mark are covered: a module whose `module_function`
# made the name private on the instance side, and a `private` sent to the
# object's own singleton class.

def try(label)
  p [label, yield]
rescue NoMethodError, NameError => e
  p [label, :raised, e.class.to_s]
end

module MFa
  module_function
  def helper = :helper
end
o = Object.new.extend(MFa)
try(:dot)      { o.helper }
try(:send)     { o.send(:helper) }
try(:public)   { o.public_send(:helper) }
try(:method)   { o.method(:helper).call }
try(:inner)    { o.instance_eval { helper } }

module Prot
  def pm = :pm
  protected :pm
end
q = Object.new.extend(Prot)
try(:prot_dot) { q.pm }
try(:prot_pub) { q.public_send(:pm) }

r = Object.new
def r.later = :later
r.singleton_class.send(:private, :later)
try(:late_dot) { r.later }
try(:late_send){ r.send(:later) }
r.singleton_class.send(:public, :later)
try(:back_dot) { r.later }
__END__
[:dot, :raised, "NoMethodError"]
[:send, :helper]
[:public, :raised, "NoMethodError"]
[:method, :helper]
[:inner, :helper]
[:prot_dot, :raised, "NoMethodError"]
[:prot_pub, :raised, "NoMethodError"]
[:late_dot, :raised, "NoMethodError"]
[:late_send, :later]
[:back_dot, :later]
