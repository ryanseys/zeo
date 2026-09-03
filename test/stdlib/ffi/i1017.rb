# Issue #1017, ported to the real ffi gem API: a block whose tail expression
# is an FFI pointer value must flow that pointer out of the block intact
# (the original guarded a codegen cast when the tail crossed the proc-return
# ABI). The yielded pointer here is NULL -- the first word of a zero-filled
# MemoryPointer -- so the pool's free list ends up holding a nil-equal
# pointer.
require "ffi"

scratch = FFI::MemoryPointer.new(16)

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

pool = Pool.new(1) { scratch.read_pointer }
if pool.first == nil
  puts "ok"
else
  puts "not_ok"
end
__END__
ok
