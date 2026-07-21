# Polymorphic (sp_RbVal) values flowing through proc parameters. A 16-byte
# sp_RbVal does not fit the 8-byte mrb_int arg slot, so a poly arg rides the
# _sp_proc_poly_args side-channel the call site publishes before the call.
# Covers single/multi poly params, mixed poly+int, poly return, and the
# re-entrancy case (a nested poly-proc call inside an arg list).
def poly(a) = a

vals = poly([1, "two", :three, 4.5])

# single poly param, poly return
id = ->(x) { x }
vals.each { |e| p id.call(e) }

# two poly params -> poly array; the first arg is itself a poly-proc call, so
# publish-after-evaluate-all is what keeps the side-channel correct here
pair = ->(x, y) { [x, y] }
p pair.call(id.call(vals[0]), vals[1])     # [1, "two"]

# mixed poly + int args in one call
tag = ->(x, n) { [x, n * 2] }
p tag.call(vals[2], 5)                      # [:three, 10]

# poly param threaded through a second proc call inside the body
wrap = ->(v) { id.call(v) }
p wrap.call(vals[3])                        # 4.5
