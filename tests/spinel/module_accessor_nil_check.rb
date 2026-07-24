# `.nil?` on a module-singleton accessor whose value type is a
# class constant: the call must check whether the slot has been
# assigned, NOT dispatch a class method `.nil?` on the resolved
# class. Pre-fix:
#
#   if (!(sp_A_cls_nil_p()))  // undeclared function — link error
#
# The single-candidate fast path in the
# `Module.accessor.<method>` resolution skipped the slot check
# and dispatched class-method on the resolved candidate. The
# slot decl emit was also gated to multi-candidate only;
# widening that gate is part of the fix.

class A
end

module Registry
  class << self
    attr_accessor :adapter
  end
end

def configure
  Registry.adapter = A
end

# Read paths — the single-candidate Class resolution would
# dispatch .nil? as a class method on A pre-fix. Now it checks
# the slot's sentinel.
def check_unset
  Registry.adapter.nil?
end

def check_after_set
  configure
  Registry.adapter.nil?
end

puts check_unset ? "nil" : "set"
puts check_after_set ? "nil" : "set"
