# `@arr = [nil] * N` against an ivar slot inferred as `poly`
# (sp_RbVal — wider than poly_array, set by `finalize_ivar_heterogeneity`
# when distinct non-array writes are observed) used to compile to
# `sp_box_int_array(sp_IntArray_new())`. The runtime storage was
# IntArray, so subsequent heterogeneous writes via the poly-recv
# `[]=` runtime widening path (or directly via `[]=` with cls_id
# dispatch) lost the typed-pointer cls_id on the IntArray-typed
# storage path.
#
# Repro: force the slot type to plain `poly` via mixed scalar
# observations. Then `[nil] * N` against the poly slot must
# produce a PolyArray, not IntArray, for cls_id-preserving
# heterogeneous reads/writes to work.

class C
  def init_arr_widen
    @arr = [nil] * 8
    @arr[0] = "string-payload"
    @arr[1] = 42
    @arr[2] = :sym
  end
  def init_str
    @arr = "scalar"
  end
  def init_int
    @arr = 7
  end
  def at(i)
    @arr[i]
  end
end

c = C.new
c.init_arr_widen
puts c.at(0).to_s   # "string-payload"
puts c.at(1).to_i   # 42
puts c.at(2).to_s   # "sym"
