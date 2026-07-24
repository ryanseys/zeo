# Issue #314 (Roundhouse warnings family): `Integer#[idx]` codegen
# fired for ANY `[]` call on an int-typed receiver, including the
# common shape:
#
#   def self.from_raw(row)
#     instance.id = row[:id] || 0
#     ...
#   end
#
# where `row` is an unpinned param whose type defaults to "int". The
# `row[:id]` then lowered to `(row >> SPS_id) & 1` — shifting a
# 64-bit int by the symbol's interned id (commonly >= 64) is
# undefined behavior under -Wshift-count-overflow, and even when it
# happens to compile the value is garbage.
#
# Fix: gate the bit-extract on the index's inferred type. A non-int
# / non-poly index (typically a Symbol or String literal) is hash /
# array subscript, never Integer-bit indexing — fall through to
# the unresolved-call placeholder so the C compile sees the right
# diagnostic instead of a warning + UB.

# Genuine bit indexing — must still work.
n = 0b1010
puts n[0]                # 0
puts n[1]                # 1
puts n[2]                # 0
puts n[3]                # 1

# Hash-subscript shape on an unpinned param — the bug's repro.
# Without the fix, `row[:k]` emits `(row >> SPS_k) & 1` and gcc/
# clang flag the shift count. With the fix, this falls through to
# the unresolved-call placeholder (the param type is unknown to
# spinel here, since the function is never called), which is the
# correct behavior — compilation succeeds without the bogus
# bit-extract.
def from_raw(row)
  row[:id] || 0
end

puts "compiled-cleanly"
