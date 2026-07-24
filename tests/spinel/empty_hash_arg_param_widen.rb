# Issue #397. `def store(receiver); receiver["k"] = "v"; end; store({})`
# previously emitted `sp_StrIntHash_set(receiver, "k", "v")` -- a const
# char* into the int-valued slot of `sp_StrIntHash`, fails C-compile.
# `{}` literal defaulted to str_int_hash, the param widened to that
# from the caller, and the body's String store didn't fit.
#
# Fix: narrow_param_hash_types_from_body_writes scans the body for
# `lv_<param>[k] = v` writes and widens the param's hash variant from
# the observed value type. Then compile_call_args_with_defaults's new
# empty_hash_coerce arm rewrites the caller's empty `{}` to emit the
# matching `sp_<HashType>_new()`. detect_poly_params also gets an
# empty-hash carve-out so it doesn't fold the body-widened concrete
# type back to poly.

def store_str(receiver)
  receiver["k"] = "v"
  receiver["k2"] = "v2"
  receiver
end

h = store_str({})
puts h["k"]                # v
puts h["k2"]               # v2

def store_sym_int(h)
  h[:a] = 1
  h[:b] = 2
  h
end

s = store_sym_int({})
puts s[:a].to_s            # 1
puts s[:b].to_s            # 2

# Mixed values widens to str_poly_hash.
def store_mixed(r)
  r["i"] = 7
  r["s"] = "x"
  r
end

m = store_mixed({})
puts m["i"].to_s           # 7
puts m["s"]                # x
