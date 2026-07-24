# Master PR #312 added heap-allocated `Method` objects but left a
# hole in heterogeneous poly_array element dispatch:
#
# - `@arr[i] = method(:foo)` mixed with `@arr[i] = some_int_array`
#   had no widen-to-poly_array path on the write side
#   (`scan_writer_calls`).
# - `finalize_ivar_heterogeneity` collapsed the multi-array
#   observation list to plain `poly` (sp_RbVal slot), discarding
#   element-array semantics.
# - `emit_poly_builtin_dispatch` had no Method-cls_id arm in the
#   read-side `bm[arg]` dispatch on poly recv.
#
# Combined effect on master: the IntArray write was void*-cast
# into a `<X>_ptr_array<Method>` storage, then the read
# dereferenced the IntArray pointer as if it were `sp_Method *`
# — `iv_fn_ptr` reads garbage and the indirect call segfaults.
# This is the natural CPU-memory dispatch shape `@fetch[a][a]`.

class C
  def peek(addr); 42; end
  def setup
    @arr = [nil] * 16
    range = [10, 20, 30, 40, 50, 60, 70, 80]
    @arr[5] = range            # IntArray
    @arr[10] = method(:peek)   # Method
  end
  def call_via(addr)
    (@arr[addr][addr & 0xff]).to_i
  end
end

c = C.new
c.setup
puts c.call_via(10)            # via Method peek_X → 42
puts c.call_via(5)             # via IntArray range → range[5] = 60
