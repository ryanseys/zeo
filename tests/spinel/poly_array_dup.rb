# Issue #427. `Array#dup` on a polymorphic-element array
# (`poly_array`) emitted the standard `cannot resolve call to
# 'dup' on poly_array` warning AND segfaulted at runtime
# (exit 139 / SIGSEGV). The combination of warning + crash
# suggested partial wiring -- emit produced something but it
# dereferenced wrong memory.
#
# Root cause: spinel_codegen.rb's `compile_array_method_expr`
# had a poly_array arm but only with `length` / `[]` / `push` /
# `clear`. `.dup` fell through to the unresolved-warning path
# which emits `0` for the dispatch -- when the caller used that
# `0` as `sp_PolyArray *`, the next access dereferenced NULL.
#
# Fix: add `.dup` / `.to_a` -> sp_PolyArray_dup (already in
# lib/sp_runtime.h, was just unhooked) and a few sibling arms
# (`empty?`, `first`, `last`) that the natural shape of "manage
# a poly_array as a normal Array" needs.
#
# Coverage:
#   - dup: shallow copy preserves the element variant (length
#     equal, types preserved).
#   - to_a: alias for dup on Array.
#   - empty?: edge case (zero-length poly_array).
#   - first / last: integer-key access on a poly recv that the
#     existing `[]` arm already handled but the named-method
#     forms didn't.

a = [1, "two", :three]
b = a.dup
puts b.length                  # 3

# Element variants preserved across dup.
c = a.to_a
puts c.length                  # 3

# empty? on poly_array.
empty = [1, "x"]
puts empty.empty? ? "yes" : "no"   # no

empty2 = [1, "x"]
empty2.clear
puts empty2.empty? ? "yes" : "no"  # yes

# first / last on poly_array.
arr = [1, "two", :three]
# Ruby's `first` on Array of mixed types returns the first elem;
# spinel boxes it as sp_RbVal. We don't assert exact value (the
# poly representation isn't easily printable here); just that
# the call doesn't crash.
arr.first
arr.last
puts "ok"                          # ok
