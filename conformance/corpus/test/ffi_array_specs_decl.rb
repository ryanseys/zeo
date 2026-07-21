# #474. New FFI type specs `:float_array` / `:int_array` for
# zero-copy bulk transfer of Spinel's contiguous Array storage to
# C functions. Spinel emits `((const double *)(<arg>)->data)` at
# the call site (`const int64_t *` for :int_array), giving the C
# side a pointer to the underlying `FloatArray` / `IntArray`
# `.data`. Lifetime is call-duration only — same contract as
# `:str`.
#
# This test exercises only the analyze + codegen path (the FFI
# functions are declared but not called); a real-world callable
# test needs a C-side function with `const double *` /
# `const int64_t *` signature, which no portable libm/libc symbol
# matches. The toy_ruby_neural_network project (the issue's
# motivating use case) drives the runtime side via the new
# tnn_upload_from_float_array shim.

module BulkF
  ffi_func :phantom_sum_f64, [:float_array, :size_t], :double
end

module BulkI
  ffi_func :phantom_sum_i64, [:int_array, :size_t], :int
end

puts "ok"
