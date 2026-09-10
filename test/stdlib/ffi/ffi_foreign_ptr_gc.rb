# A foreign pointer (malloc'd C memory) kept alive in an ivar must not be
# traced by the GC as a heap object. Heavy allocation churns GC cycles that
# scan the holder; a collector that followed the foreign pointer would crash.
# The foreign address comes from libc malloc through attach_function.
require "ffi"

module F
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :malloc, [:size_t], :pointer
  attach_function :free,   [:pointer], :void
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
  holders.push(Holder.new(F.malloc(64)))
  n += 1
end

i = 0
while i < 200000
  s = "row-" + i.to_s
  i += 1
end
puts "ok #{holders.length}"

holders.each { |h| F.free(h.p) }
__END__
ok 200
