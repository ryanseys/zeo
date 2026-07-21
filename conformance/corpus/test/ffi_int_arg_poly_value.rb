# Issue #626 sub-issue 1. FFI numeric arg (`:int`, `:uint32`,
# `:float`, ...) call site receiving an sp_RbVal value previously
# emitted `(int)(<struct>)` — gcc rejected it with "aggregate
# value used where an integer was expected". Fix: at the catch-
# all numeric coercion in compile_ffi_func_call, extract `.v.i`
# (or `.v.f` for `:float` / `:double`) when the arg's static
# type is poly. Mirrors the existing :str / :ptr poly handling.
#
# The original report involved a cross-class attr_writer widening
# (`Mat#@nrows` widened to sp_RbVal when a sibling class's
# poly-recv `obj.nrows = val` site fired); this regression test
# exercises the same symptom shape through a within-class poly
# slot created by a mixed-type initializer.

class Sink
  attr_accessor :n
  def initialize(flag)
    @n = flag ? 42 : "fallback"
  end
end

module Lib
  ffi_func :abs, [:int], :int
end

s = Sink.new(true)
puts Lib.abs(s.n)
