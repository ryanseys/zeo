# Issue #129: ENV["X"] was inferred as int at every call site even though
# the dispatch correctly emits sp_str_dup_external(getenv(...)) returning
# const char *. Fix is in `infer_operator_type`'s `[]` branch (early
# ENV check) plus `compile_eq` for the `string == string` case (NULL-safe
# via the new sp_str_eq runtime helper, since strcmp(NULL, ...) is UB and
# segfaults on `"1" == ENV["UNSET"]`).
#
# Tests pin the four shapes from the issue body, plus the reverse-order
# segfault case Sam called out.

# Use a variable that's vanishingly unlikely to be set in any environment.
UNSET = "SPINEL_TEST_VAR_THAT_DOES_NOT_EXIST_129"

# 1. nil? on unset → true. Was: constant-folded to false at codegen.
puts ENV[UNSET].nil?                      # true

# 2. local capture + nil? roundtrip on unset → true.
s = ENV[UNSET]
puts s.nil?                                # true

# 3. inline puts on unset → blank line. Was: printed pointer-as-int.
puts ENV[UNSET]                            # (blank)

# 4. equality on unset → false. Was: pointer-int compare, also always false
#    but for the wrong reason (and would crash if the integer happened to
#    be a valid pointer prefix).
puts ENV[UNSET] == "1"                     # false

# 5. Reverse-order equality on unset → false. Was: segfault. lt=string,
#    at=int → fell through to strcmp("1", <int>), dereferencing the int
#    as a pointer.
puts "1" == ENV[UNSET]                     # false

# 6. nil? on a likely-set var (PATH is universal on POSIX). The bootstrap
#    needs PATH to find cc; if this test runs at all PATH is set. Pins the
#    "set var" branch of the same fix.
puts ENV["PATH"].nil?                      # false
