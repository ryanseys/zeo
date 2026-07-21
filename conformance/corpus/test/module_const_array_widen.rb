# Issue #333: a module-level array constant initialized as an empty
# literal (`LOG = []`) was typed as `sp_IntArray *`, regardless of
# how it was used. A later `LOG << some_hash` then emitted
# `sp_IntArray_push(cst_M_LOG, hash_ptr)`, mistyping the pointer as
# mrb_int.
#
# Fix: extend the existing `refine_module_ivar_types` pass (which
# already handled `@slots = {}` shape per #303) to also walk
# `ConstantWriteNode` declarations like `LOG = []`. The companion
# scanner `scan_module_const_writes` walks the module's class
# methods for `LOG << v` / `LOG[k] = v` / `LOG.push(v)` shapes
# rooted at a `ConstantReadNode` receiver. Body locals (`entry =
# {...}; LOG << entry`) are also declared in the temporary scope so
# `infer_type` resolves the pushed value's actual hash/obj type
# instead of the int default.
#
# Surfaced via Roundhouse's `module Broadcasts; LOG = []; ... LOG <<
# entry` pattern.

module Broadcasts
  module_function
  LOG = []

  def record(action)
    entry = { action: action, target: "x" }
    LOG << entry
  end

  def size
    LOG.length
  end
end

Broadcasts.record(:append)
Broadcasts.record(:replace)
puts Broadcasts.size           # 2
