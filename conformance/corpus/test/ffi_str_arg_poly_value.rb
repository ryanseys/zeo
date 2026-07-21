# #502. FFI `:str` arg call site receiving an sp_RbVal value
# (e.g. an ivar widened to poly by a ternary) used to emit the
# raw struct as the call arg and failed C compile with
# "passing 'sp_RbVal' to parameter of incompatible type
# 'const char *'". Fix: at the :str / :ptr coercion in
# compile_ffi_func_call, extract `.v.s` (or `.v.p`) when the
# arg's static type is poly.
#
# Test uses libc's `atoi(const char *)` to exercise the :str
# coercion through a real, always-available external function;
# the value (5) doesn't matter — what matters is that the C
# call site emits cleanly under -Werror.

class Sink
  attr_accessor :body
  def initialize(flag)
    @body = flag ? "42" : 7
  end
end

module Lib
  ffi_func :atoi, [:str], :int
end

s = Sink.new(true)
puts Lib.atoi(s.body)
