# A gem that wraps `FFI::Struct` once and declares every real struct against
# the wrapper. gssapi does exactly this -- `GssUMStruct < FFI::Struct` exists
# only to hide `[]`/`[]=`, and nothing else in the gem names `FFI::Struct`
# again -- so zeo, which recognized only a DIRECT superclass, saw none of them
# as structs at all and reported the grandchildren as undeclared FFI types.
#
# A subclass of a struct class is a struct class. `FFI::ManagedStruct` joins
# the same set: its auto-release finalizer is a lifetime concern, not an ABI
# one, so the layout reads identically.
#
# What is NOT inherited is the LAYOUT. `class B < A; end` leaves `B.size` at 0
# in ruby-ffi and `B.new` raises "no Struct layout configured" -- a subclass
# gets its parent's accessors and none of its fields.
require "ffi"

module Lib
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class Wrapper < FFI::Struct
    def summary = "#{self.class.name} holds #{self.class.size} bytes"
  end

  class Timeval < Wrapper
    layout :tv_sec, :long, :tv_usec, :long
  end

  # Two steps down, with its own layout, and taken by REFERENCE in a
  # signature -- which is what a bare struct class means to ruby-ffi.
  class Stamp < Timeval
    layout :sec, :long, :usec, :long
  end

  attach_function :gettimeofday, [Stamp, :pointer], :int
end

s = Lib::Stamp.new
p Lib.gettimeofday(s, nil)
p s[:sec] > 0
p Lib::Stamp.size
p s.summary
__END__
0
true
16
"Lib::Stamp holds 16 bytes"
