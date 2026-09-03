# `respond_to?` and `defined?(obj.m)` answer true for a PRIVATE per-object
# singleton method, where CRuby answers false and nil.
#
# `responds_to_value` short-circuits on
# `object_has_singleton_method(recv, name)` and returns true without ever
# asking what visibility that row carries -- the class-side walk below it
# applies the `include_all` rule, the per-object probe above it does not.
# `defined?(obj.m)` reaches the same probe through `responds_to_or_missing`.

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
try(:respond)     { o.respond_to?(:helper) }
try(:respond_all) { o.respond_to?(:helper, true) }
try(:defined)     { defined?(o.helper) }

r = Object.new
def r.later = :later
r.singleton_class.send(:private, :later)
try(:r_respond)     { r.respond_to?(:later) }
try(:r_respond_all) { r.respond_to?(:later, true) }
try(:r_defined)     { defined?(r.later) }
r.singleton_class.send(:public, :later)
try(:r_back)        { r.respond_to?(:later) }

module Prot
  def pm = :pm
  protected :pm
end
q = Object.new.extend(Prot)
# A PROTECTED one is hidden from the default `respond_to?` too -- CRuby's
# rule is "public only" unless `include_all`.
try(:prot_respond)  { q.respond_to?(:pm) }
__END__
[:respond, false]
[:respond_all, true]
[:defined, nil]
[:r_respond, false]
[:r_respond_all, true]
[:r_defined, nil]
[:r_back, true]
[:prot_respond, false]
