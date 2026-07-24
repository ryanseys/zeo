# A format() argument that is itself a call rooting its own operands used to
# leak that rooting decl into the PolyArray_push argument position
# (`sp_PolyArray_push(_t, sp_RbVal _t2 = ...;` -- illegal C, #1508/#1498).
def f(v) = v * 1000.0
puts format("L: %.3f; A: %.3f; X: %.3f", f(0.001), f(0.0025), f(0.5))
puts format("%d/%s", f(2).to_i, "ok")
