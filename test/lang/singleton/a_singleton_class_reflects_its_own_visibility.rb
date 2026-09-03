# A singleton class's instance-method reflection reports every per-object row
# as PUBLIC, whatever mark the object carries.
#
# `sc.instance_methods(false)`, `sc.private_instance_methods(false)`,
# `sc.method_defined?` and `sc.private_method_defined?` all resolve through
# the singleton class ID. The rows themselves live in the identity-keyed
# per-object tables, and so does the mark, so a class-id walk sees the names
# but not their visibility.
#
# The dispatch half is `an_explicit_receiver_honours_a_singleton_visibility`;
# this is the reflection half of the same missing dimension.

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
sc = o.singleton_class
try(:im)        { sc.instance_methods(false).sort }
try(:pim)       { sc.private_instance_methods(false) }
try(:pubim)     { sc.public_instance_methods(false).sort }
try(:protim)    { sc.protected_instance_methods(false) }
try(:md_pub)    { sc.method_defined?(:pub) }
try(:md_hid)    { sc.method_defined?(:hid) }
try(:pmd_hid)   { sc.private_method_defined?(:hid) }
try(:pmd_pub)   { sc.private_method_defined?(:pub) }
try(:prmd_prot) { sc.protected_method_defined?(:prot) }
try(:im_named)  { sc.instance_method(:hid).name }

r = Object.new
def r.later = :later
rsc = r.singleton_class
rsc.send(:private, :later)
try(:r_im)      { rsc.instance_methods(false) }
try(:r_pim)     { rsc.private_instance_methods(false) }
try(:r_pmd)     { rsc.private_method_defined?(:later) }
__END__
[:im, [:prot, :pub]]
[:pim, [:hid]]
[:pubim, [:pub]]
[:protim, [:prot]]
[:md_pub, true]
[:md_hid, false]
[:pmd_hid, true]
[:pmd_pub, false]
[:prmd_prot, true]
[:im_named, :hid]
[:r_im, []]
[:r_pim, [:later]]
[:r_pmd, true]
