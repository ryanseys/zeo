# An FFI :string arg call site receiving a poly-typed value (an ivar widened
# by a mixed-type initializer) must coerce the boxed string rather than
# passing the box itself. Ported from spinel's ffi_func (#502) to the real
# ffi gem API; libc's atoi(const char *) exercises the :string coercion
# through a real, always-available external function.
require "ffi"

class Sink
  attr_accessor :body
  def initialize(flag)
    @body = flag ? "42" : 7
  end
end

module Lib
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :atoi, [:string], :int
end

s = Sink.new(true)
puts Lib.atoi(s.body)
__END__
42
