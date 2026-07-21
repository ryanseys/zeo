# A foreign pointer (here an ffi_buffer address) kept alive in an ivar must not
# be traced by the GC as a heap object. Heavy allocation churns GC cycles that
# scan the holder; before the fix the collector followed the foreign pointer
# and crashed. (Run under SPINEL_GC_STRESS=1/SPINEL_GC_VERIFY=1 to make it
# deterministic.)
module F
  ffi_lib "c"
  ffi_buffer :buf, 64
end

class Holder
  def initialize(p)
    @p = p
  end
  def p; @p; end
end

holders = []
n = 0
while n < 200
  holders.push(Holder.new(F.buf))
  n += 1
end

i = 0
while i < 200000
  s = "row-" + i.to_s
  i += 1
end
puts "ok #{holders.length}"
