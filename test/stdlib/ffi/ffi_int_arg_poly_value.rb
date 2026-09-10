# An FFI numeric arg (:int) call site receiving a poly-typed value (an ivar
# widened by a mixed-type initializer) must coerce the boxed integer rather
# than passing whatever wrapper it arrived in.
require "ffi"

class Sink
  attr_accessor :n
  def initialize(flag)
    @n = flag ? 42 : "fallback"
  end
end

module Lib
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :abs, [:int], :int
end

s = Sink.new(true)
puts Lib.abs(s.n)
__END__
42
