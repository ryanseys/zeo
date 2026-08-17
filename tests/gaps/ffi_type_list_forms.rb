# GAP -- imported from the spinel corpus at c55d9bdb.
# zeo requires attach_function's argument list to be a LITERAL array; ruby
# accepts any expression, including a constant and `[:float] * 2`.
#
# An ffi type list may be written the way a real adapter writes a long one: a
# constant, or `[...] * n`. Every form names the same function as the literal.
#
# Ported from spinel's ffi_type_list_forms, whose `ffi_func` class macro is
# compile-time spinel DSL with no CRuby analog. The real ffi gem spells it
# `attach_function`, which takes the same argument-type array -- so the forms
# under test (a frozen constant, a repeated array, a literal) carry over
# unchanged.
require "ffi"

module Demo
  module Ext
    extend FFI::Library
    ffi_lib "m"

    TYPES = [:float].freeze
    attach_function :fabsf, TYPES, :float
    attach_function :fmaxf, [:float] * 2, :float
    attach_function :fminf, [:float, :float], :float
  end
end

puts Demo::Ext.fabsf(-2.5)
puts Demo::Ext.fmaxf(1.5, 2.5)
puts Demo::Ext.fminf(1.5, 2.5)
