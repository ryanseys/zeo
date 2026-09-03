# `Object#methods` and `#public_methods` report only what the receiver's CLASS
# chain carries, so a method installed on the OBJECT is missing from both --
# and a private one wrongly shows up in `public_methods`.
#
# All three rows read `instance_method_names(recv.class_id(), filter, inherit)`
# (`builtins/kernel.rs`), which walks a class id. A per-object singleton row
# sits AHEAD of that chain and is keyed by identity, so no class-id walk can
# reach it. `singleton_methods` is the only row that asks by identity.

def try(label)
  p [label, yield]
rescue NoMethodError, NameError => e
  p [label, :raised, e.class.to_s]
end

o = Object.new
class << o
  def pub = :pub
  private
  def hid = :hid
  protected
  def prot = :prot
end
try(:methods_pub)   { o.methods.include?(:pub) }
try(:methods_prot)  { o.methods.include?(:prot) }
try(:methods_hid)   { o.methods.include?(:hid) }
try(:public_pub)    { o.public_methods.include?(:pub) }
try(:public_hid)    { o.public_methods.include?(:hid) }
try(:public_prot)   { o.public_methods.include?(:prot) }
try(:public_false)  { o.public_methods(false).sort }
try(:private_false) { o.private_methods(false).include?(:hid) }
try(:prot_false)    { o.protected_methods(false) }

module MFa
  module_function
  def helper = :helper
end
e = Object.new.extend(MFa)
try(:ext_methods)   { e.methods.include?(:helper) }
try(:ext_private)   { e.private_methods(false).include?(:helper) }
__END__
[:methods_pub, true]
[:methods_prot, true]
[:methods_hid, false]
[:public_pub, true]
[:public_hid, false]
[:public_prot, false]
[:public_false, [:pub]]
[:private_false, true]
[:prot_false, [:prot]]
[:ext_methods, false]
[:ext_private, true]
