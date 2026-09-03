# A bare `FFI::Struct` subclass in an `attach_function` signature is
# ruby-ffi's StructByReference -- the ABI type is a POINTER (`.by_value` is
# the by-value spelling). An argument takes a struct instance and passes its
# backing memory; a return comes back as a plain `FFI::Pointer` (the gem
# does NOT auto-wrap it). gmtime/timegm round-trip both directions.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class Tm < FFI::Struct
    layout :tm_sec, :int,
           :tm_min, :int,
           :tm_hour, :int,
           :tm_mday, :int,
           :tm_mon, :int,
           :tm_year, :int,
           :tm_wday, :int,
           :tm_yday, :int,
           :tm_isdst, :int,
           :tm_gmtoff, :long,
           :tm_zone, :pointer
  end

  attach_function :gmtime, [:pointer], Tm
  attach_function :timegm, [Tm], :long
end

t = FFI::MemoryPointer.new(:long)
t.write_long(86_400 * 365)
ptr = LibC.gmtime(t)
p ptr.class
p ptr.null?
tm = LibC::Tm.new(ptr)
p tm[:tm_year] + 1900
p tm[:tm_mon]
p tm[:tm_mday]
p tm[:tm_hour]
p LibC.timegm(tm)
puts "still running"
__END__
FFI::Pointer
false
1971
0
1
0
31536000
still running
