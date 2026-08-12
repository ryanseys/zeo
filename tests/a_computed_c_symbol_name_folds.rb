# A library whose windows entry points carry a suffix writes the suffix once
# at the top of the module and builds every C symbol out of it --
# `attach_function :list_readers, 'SCardListReaders' + str_suffix, ...`, which
# smartcard does nine times. The suffix is the same compile-time platform
# question the guards fold, so the symbol name is a compile-time fact too.
#
# A plain READ of the local is not what stops the fold: nine declarations
# share one suffix, and each of those reads is still the value the assignment
# gave it. Only a rebind or an in-place mutation makes the value unreadable.
require "ffi"

module L
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  suffix = FFI::Platform.windows? ? "_s" : ""
  prefix = "str"

  attach_function :length, prefix + "len" + suffix, [:string], :size_t
  attach_function :compare, prefix + "cmp" + suffix, [:string, :string], :int
  # The 3-argument form computes the ruby name from the same string.
  attach_function "strlen", [:string], :size_t
end

p L.length("hello")
p L.compare("a", "a")
p L.compare("a", "b") < 0
p L.strlen("worlds")

# A local the body REBINDS between declarations reads as whatever the
# preceding assignment left, exactly like ruby.
module M
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  which = "len"
  attach_function :first, "str" + which, [:string], :size_t
  which = "cmp"
  attach_function :second, "str" + which, [:string, :string], :int
end

p M.first("four")
p M.second("z", "z")
puts "still running"
