# An FFI :string arg call site receiving a poly-typed value (an ivar widened
# by a mixed-type initializer) must coerce the boxed string rather than
# passing whatever wrapper it arrived in. libc's atoi(const char *) is the
# external function, so the coercion is exercised against a real one.
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
