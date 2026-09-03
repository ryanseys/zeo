# An FFI numeric arg (:int) call site receiving a poly-typed value (an ivar
# widened by a mixed-type initializer) must coerce the boxed integer rather
# than passing the box itself. Ported from spinel's ffi_func (#626) to the
# real ffi gem API; the symptom shape -- a within-class poly slot created by
# a mixed-type initializer -- is preserved.
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
