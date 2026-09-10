# An ffi type list may be written the way a real adapter writes a long one:
# a constant, a repeated array, a word list, any of them frozen or
# parenthesised. Every form names the same function as the plain literal.

require "ffi"

module Demo
  module Ext
    extend FFI::Library
    ffi_lib "m"

    TYPES = [:float].freeze
    PAIR = [:float, :float]
    NESTED = ([:float] * 2).freeze
    WORDS = %i[float float]
    ALIAS = TYPES

    attach_function :fabsf, TYPES, :float
    attach_function :truncf, [:float].freeze, :float
    attach_function :ceilf, ALIAS, :float
    attach_function :fmaxf, [:float] * 2, :float
    attach_function :fminf, [:float, :float], :float
    attach_function :hypotf, PAIR, :float
    attach_function :powf, NESTED, :float
    attach_function :fdimf, WORDS, :float
    attach_function :copysignf, [:float].freeze * 2, :float
  end
end

e = Demo::Ext
p [e.fabsf(-2.5), e.truncf(2.9), e.ceilf(2.1)]
p [e.fmaxf(1.5, 2.5), e.fminf(1.5, 2.5), e.hypotf(3.0, 4.0)]
p [e.powf(2.0, 3.0), e.fdimf(5.0, 2.0), e.copysignf(3.0, -1.0)]

# A list the class body REBINDS between declarations is not one value, so the
# read is refused rather than answered with a stale one.
module Demo
  module Late
    extend FFI::Library
    ffi_lib "m"
    LATER = [:double]
    attach_function :fabs, LATER, :double
  end
end
p Demo::Late.fabs(-3.25)
__END__
[2.5, 2.0, 3.0]
[2.5, 1.5, 5.0]
[8.0, 3.0, -3.0]
3.25
