#@ only: macos
# An `ffi_lib` whose candidates zeo cannot fold (an ENV read, a local, a
# splat) is not a rejection: the expressions evaluate when the class body
# executes, with the gem's exact shape -- every top-level argument is its
# own required library, an Array argument lists alternatives for one, and a
# library that can't open is CRuby's require-time LoadError at that very
# statement. Every attach_function under it resolves through the handles.
require "ffi"

module RtMath
  extend FFI::Library
  lib = ENV["ZEO_TEST_NO_SUCH_ENV"] || "m"
  ffi_lib lib
  attach_function :fabs, [:double], :double
end
puts RtMath.fabs(-3.5)

module Alternatives
  extend FFI::Library
  candidates = ["zeo-definitely-absent", "m"]
  ffi_lib candidates
  attach_function :my_floor, :floor, [:double], :double
end
puts Alternatives.my_floor(2.75)

module Splatted
  extend FFI::Library
  libs = ["m", "c"]
  ffi_lib(*libs)
  attach_function :my_ceil, :ceil, [:double], :double
end
puts Splatted.my_ceil(2.25)

begin
  module Nope
    extend FFI::Library
    ffi_lib "zeo-definitely-absent-#{Process.pid}"
  end
rescue LoadError => e
  puts "raised #{e.class} at ffi_lib"
end
__END__
3.5
2.0
3.0
raised LoadError at ffi_lib
