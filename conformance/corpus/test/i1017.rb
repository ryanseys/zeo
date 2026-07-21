# Issue #1017: a proc body whose tail expression is an FFI :ptr value
# wasn't cast to (mrb_int)(uintptr_t) before being returned through the
# proc-fn `mrb_int` ABI slot. The existing #709 cast bridge (in
# compile_proc_literal) guarded on `type_is_pointer == 1`, but FFI :ptr
# is intentionally non-GC and reports 0 there, so the cast was skipped
# and cc failed with -Wint-conversion.
#
# Repro: a yielding constructor whose block tail is an `ffi_read_ptr`
# call. Without the fix, cc rejects `return ((void *)(...))` from a
# `mrb_int`-returning `_sp_proc_fn_N`. With the fix, the read yields
# NULL (BSS-zero buffer) and the pool's free list ends up holding nil.

module Buf
  ffi_buffer :scratch, 16
  ffi_read_ptr :first_ptr, 0
end

class Pool
  def initialize(n)
    @free = []
    i = 0
    while i < n
      @free.push(yield)
      i += 1
    end
  end
  def first
    @free[0]
  end
end

p = Pool.new(1) { Buf.first_ptr(Buf.scratch) }
if p.first == nil
  puts "ok"
else
  puts "not_ok"
end
