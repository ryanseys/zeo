# #518. `M.accessor.delete` previously emitted a "cannot resolve
# call to 'adapter' on class" warning even after #511. Root: the
# statement-level `delete` handler in compile_call_stmt called
# `compile_expr_gc_rooted(recv)` unconditionally *before* checking
# whether recv's type matched any of its hash / array arms. The
# accessor read's compile fired the obj-method-dispatch warn at
# the bottom of compile_call_expr; the outer const-fold path then
# still emitted the correct call but the spurious warning had
# already landed.
#
# Fix: defer the compile_expr_gc_rooted into each matched arm so
# non-container receivers fall through cleanly to the general
# call dispatch.

module M
  class << self
    attr_accessor :adapter
  end
end

module Backend
  def self.delete
    "ran-delete"
  end
end

M.adapter = Backend
puts M.adapter.delete

# Also exercise statement context (no puts) to mirror the issue
# repro shape — must not crash and must not warn.
M.adapter.delete
