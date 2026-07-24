# Issue #412. An empty array literal followed by a `<<` push
# inside a loop, where the pushed value rides through an
# intermediate variable, emitted as `sp_IntArray *` even though
# the pushed values were class pointers:
#
#   def make
#     results = []
#     i = 0
#     while i < 3
#       instance = Foo.new
#       results << instance        # ← intermediate var
#       i += 1
#     end
#     results
#   end
#
#   error: passing argument 2 of 'sp_IntArray_push' makes integer
#          from pointer without a cast
#     sp_IntArray_push(lv_results, lv_instance);
#                                  ^~~~~~~~~~~
#
# Three combinations distinguished:
#
#   - direct push (`results << Foo.new`)            -- worked
#   - intermediate var, no loop                     -- worked
#   - intermediate var INSIDE loop                  -- failed
#
# Root cause: `scan_locals`'s `<<` push observation upgrades
# `int_array → <typed>_array` only when `infer_type(arg)` resolves
# to a typed value. In pass 1, the intermediate local
# (`instance`) hadn't been declared in the scope yet, so
# `infer_type(LocalVariableReadNode "instance")` fell back to
# "int" via `find_var_type` returning "" — push didn't upgrade.
# Pass 2 (with locals declared) DID resolve and DID upgrade
# `ltypes2[results]` to `obj_Foo_ptr_array`, but
# `refine_method_body_locals`'s pass-1↔pass-2 merge only handled
# `int → concrete` and `nil → nullable_pointer`, not
# `int_array → typed_array`. The pass-2 result was dropped.
#
# Compounding the local-decl miss: `infer_all_returns`'s
# top-level-method branch ran a single-pass `scan_locals` before
# calling `infer_body_return`, so the function's return type was
# also pinned at `int_array` independently of what
# precompute_all_scope_decls eventually computed for the local.
# Both branches now route through `refine_method_body_locals`.
#
# Coverage:
#   - the canonical Rails-style `_adapter_all` shape from the
#     issue's repro,
#   - a typed-array (str) variant to prove the pass-2 merge
#     covers `int_array → str_array` too,
#   - a baseline direct-push variant as a regression check that
#     the previously-working path still works.

class Foo
  attr_accessor :v
  def initialize
    @v = 0
  end
end

def make_with_intermediate
  results = []
  i = 0
  while i < 3
    instance = Foo.new
    results << instance
    i += 1
  end
  results
end

a = make_with_intermediate
puts a.length                   # 3

def make_strs_with_intermediate
  buf = []
  i = 0
  while i < 4
    s = "row" + i.to_s
    buf << s
    i += 1
  end
  buf
end

s = make_strs_with_intermediate
puts s.length                   # 4
puts s[0]                       # row0
puts s[3]                       # row3

# Baseline direct-push (was already working).
def make_direct
  results = []
  i = 0
  while i < 3
    results << Foo.new
    i += 1
  end
  results
end

puts make_direct.length         # 3
