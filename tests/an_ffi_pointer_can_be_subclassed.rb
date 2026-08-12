# `class Handle < FFI::AutoPointer` with a `self.release` is the ffi gem's
# own idiom for a C handle -- the single biggest subclassing ask in the
# gem-probe ledger. Both pointer classes are payload roots now, so the
# subclass is a `ValueSubclass` around a pointer payload and inherits the
# whole `FFI::Pointer` read/write surface through the ancestry.
#
# What zeo does NOT do is run `self.release`: there is no finalizer, so a
# handle that closes an OS resource stays open until the process exits.
# Nothing LEAKS that Rust would not also free (an owned buffer is reference
# counted) -- see `ext/ffi/auto_pointer.rs`.
require "ffi"

class Handle < FFI::AutoPointer
  def self.release(ptr)
    $released = ptr.address
  end

  def tag = :handle
end

h = Handle.new(FFI::Pointer.new(0x1234))
p h.class
p Handle.ancestors.take(4)
p h.address
p h.tag
p h.is_a?(FFI::Pointer)
p h.null?
# `#autorelease?` is the gem's default for this class -- true here, and
# false for a raw pointer, which owns nothing.
p h.autorelease?
h.autorelease = false
p h.autorelease?
p FFI::Pointer.new(4).autorelease?
p FFI::MemoryPointer.new(:int, 2).autorelease?

# A plain `FFI::Pointer` subclass, with ivars and an `initialize` that
# seats the real payload through `super`.
class Slot < FFI::Pointer
  def initialize(address, label)
    super(address)
    @label = label
  end

  attr_reader :label
end

s = Slot.new(0x40, "meta")
p s.class
p s.address
p s.label
p s.null?
p Slot.new(0, "zero").null?

# The inherited read/write surface works on the subclass instance.
buf = FFI::MemoryPointer.new(:int, 1)
buf.write_int(4242)
view = Slot.new(buf.address, "view")
p view.read_int
puts "still running"
