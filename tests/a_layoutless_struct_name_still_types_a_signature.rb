# A `class X < FFI::Struct` whose layout the compiler never saw (ffi_dry's
# `dsl_layout` builds one at runtime) still NAMES a struct: a signature
# position only needs the by-reference fact, so `attach_function` typing
# works -- the return is the plain `FFI::Pointer` StructByReference always
# produces, and a layout-carrying class can view it.
require "ffi"

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  class OpaqueTm < FFI::Struct
  end

  class Tm < FFI::Struct
    layout :tm_sec, :int,
           :tm_min, :int,
           :tm_hour, :int,
           :tm_mday, :int,
           :tm_mon, :int,
           :tm_year, :int
  end

  attach_function :gmtime, [:pointer], OpaqueTm
end

t = FFI::MemoryPointer.new(:long)
t.write_long(86_400 * 365)
ptr = LibC.gmtime(t)
p ptr.class
p ptr.null?
tm = LibC::Tm.new(ptr)
p tm[:tm_year] + 1900
p tm[:tm_mday]
puts "still running"
