# `lambda {}` / `proc {}` (method-call form) record their block's return
# type so `.call` unboxes the sp_proc_call mrb_int result back to the
# real type instead of rendering the pointer as an integer.
f = lambda { "hello" }
puts f.call

g = proc { "world" }
puts g.call

# Pointer (array) return through the proc-call ABI.
h = lambda { [1, 2, 3] }
p h.call

# A rescue-bound exception value flows out through the same path.
r = lambda { begin; raise "boom"; rescue => e; "msg:#{e.message}"; end }
puts r.call

# Int return still works (no cast, ABI passes mrb_int through).
n = proc { 42 }
puts n.call
