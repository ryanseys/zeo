# #531. Sibling to #513. The poly-recv narrowing in af55659
# walks the global AST for `<Class>.new(...)` call sites to
# recover concrete classes seen at a poly ivar slot. The walker
# matched only `ConstantReadNode` receivers, so module-scoped
# classes (`module M; class Holder; end; end` with call sites
# `M::Holder.new(...)`) fell through to the all-classes fallback.
# Server got pulled into M::Holder.use's cls_id-dispatch table
# even though no M::Server is ever assigned to @w, and
# M::Server#run's port widened to sp_RbVal.
#
# Fix: also match `ConstantPathNode` receivers via
# `const_ref_flat_name`, which produces the same flattened
# `Mod_Class` form that `@cls_names` stores.

module M
  class WorkerA
    def run(item); item + "!"; end
  end
  class WorkerB
    def run(item); item + "?"; end
  end
  class Holder
    def initialize(w); @w = w; end
    def use(item); @w.run(item); end
  end
  # Same shape as #513's top-level Server: same method name (`run`),
  # different param type (int instead of string). After the fix,
  # M::Server#run keeps `mrb_int lv_port` and M::Server is dropped
  # from the M::Holder.use dispatch table.
  class Server
    def run(port); port + 1; end
  end
end

puts M::Holder.new(M::WorkerA.new).use("x")
puts M::Holder.new(M::WorkerB.new).use("y")
puts M::Server.new.run(80)
