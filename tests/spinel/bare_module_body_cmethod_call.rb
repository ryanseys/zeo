# Bare (receiverless) class-method call in a module/class body must propagate
# the callee's return type to the call expression.
#
# `def self.mk` returns a typed hash; called bare as `mk` (not `M.mk`) in the
# module body and passed as an argument, its result previously typed `void` --
# the bare-call resolution read the scope-pass-only global g_cbody_class_id
# (unset during the inference fixpoint) instead of the per-node enclosing-cbody
# (node_cbody[id]). The void arg then made `take`'s param unknown and the call
# miscompiled (`error: variable or field declared void`). Now it resolves via
# node_cbody[id], so `take(mk)` sees a Hash[String,String].

module M
  def self.mk
    h = {"" => ""}
    h.delete("")
    h["a"] = "1"
    h
  end

  class R
    def take(opts)
      n = 0
      opts.each { |k, v| n = n + 1 }
      n
    end
  end

  COUNT = R.new.take(mk)   # bare `mk`; result must be Hash[String,String], not void
end

puts M::COUNT.to_s         # 1
