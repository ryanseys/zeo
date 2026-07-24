# An arrow lambda (`-> {}`, the sp_Val / sp_lam_* ABI, distinct from the
# `lambda {}` proc-call ABI) returning a string literal round-trips its
# value instead of the old hardcoded sp_lam_int(0) stub.
f = -> { "arrow" }
puts f.call

g = -> { 7 }
puts g.call
