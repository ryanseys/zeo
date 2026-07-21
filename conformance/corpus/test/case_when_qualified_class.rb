# `case obj when Mod::Klass` had two bugs:
#
# 1. compile_case_stmt emitted the predicate tmp as `sp_<X> *` even
#    when the predicate's actual storage was a value type
#    (`sp_<X>` without the pointer) — producing C errors of the
#    form "initializing 'sp_X *' with an expression of incompatible
#    type 'sp_X'; take the address with &". The fix routes the
#    declaration through c_type so value-type classes get the
#    right C shape.
#
# 2. The static-resolution branch in compile_when_conds only matched
#    ConstantReadNode arguments, so qualified constants
#    (ConstantPathNode like `Mod::Klass`) fell through to the
#    poly-receiver path and emitted `<undeclared identifier>` C.
#    Fix: also accept ConstantPathNode and route through
#    resolve_const_ref_name to get the registered class name.

module Mod
  class Klass
    def initialize(v); @v = v; end
    def get; @v; end
  end
  class Other
    def initialize(v); @v = v; end
    def get; @v; end
  end
end

k = Mod::Klass.new(1)
result = case k
         when Mod::Klass then "match"
         when Mod::Other then "other"
         else "neither"
         end
puts result

# Plain (non-namespaced) class also exercises the c_type fix, since
# the pointer-vs-value mismatch was independent of namespacing.
class Plain
  def initialize; @x = 1; end
end
p_inst = Plain.new
result2 = case p_inst
          when Plain then "plain"
          else "?"
          end
puts result2
