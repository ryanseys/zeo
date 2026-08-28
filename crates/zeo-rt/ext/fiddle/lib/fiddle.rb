# frozen_string_literal: true

# zeo-authored entry point. Upstream fiddle.rb branches on RUBY_ENGINE
# between the C extension (`fiddle.so`) and the pure-Ruby FFI backend; zeo
# always takes the FFI backend (over its own native FFI), so the branch is
# gone. The `TYPE_*` constants are written out explicitly -- upstream copies
# them from Fiddle::Types with a `const_set` loop, which a computed constant
# name can't survive AOT compilation.
require 'fiddle/ffi_backend'
require 'fiddle/closure'
require 'fiddle/function'
require 'fiddle/version'

module Fiddle
  def self.last_error
    FFI.errno
  end

  def self.last_error=(error)
    FFI.errno = error || 0
  end

  # call-seq: dlopen(library) => Fiddle::Handle
  #
  # Creates a new handler that opens +library+, and returns an instance of
  # Fiddle::Handle. If +nil+ is given, the default handle (every library
  # already loaded) is opened -- most libc functions resolve through it.
  def dlopen(library)
    Fiddle::Handle.new(library)
  end
  module_function :dlopen

  RTLD_GLOBAL = Handle::RTLD_GLOBAL # :nodoc:
  RTLD_LAZY   = Handle::RTLD_LAZY   # :nodoc:
  RTLD_NOW    = Handle::RTLD_NOW    # :nodoc:

  TYPE_VOID         = Types::VOID
  TYPE_VOIDP        = Types::VOIDP
  TYPE_CHAR         = Types::CHAR
  TYPE_UCHAR        = Types::UCHAR
  TYPE_SHORT        = Types::SHORT
  TYPE_USHORT       = Types::USHORT
  TYPE_INT          = Types::INT
  TYPE_UINT         = Types::UINT
  TYPE_LONG         = Types::LONG
  TYPE_ULONG        = Types::ULONG
  TYPE_LONG_LONG    = Types::LONG_LONG
  TYPE_ULONG_LONG   = Types::ULONG_LONG
  TYPE_FLOAT        = Types::FLOAT
  TYPE_DOUBLE       = Types::DOUBLE
  TYPE_VARIADIC     = Types::VARIADIC
  TYPE_CONST_STRING = Types::CONST_STRING
  TYPE_BOOL         = Types::BOOL
  TYPE_INT8_T       = Types::INT8_T
  TYPE_UINT8_T      = Types::UINT8_T
  TYPE_INT16_T      = Types::INT16_T
  TYPE_UINT16_T     = Types::UINT16_T
  TYPE_INT32_T      = Types::INT32_T
  TYPE_UINT32_T     = Types::UINT32_T
  TYPE_INT64_T      = Types::INT64_T
  TYPE_UINT64_T     = Types::UINT64_T
  TYPE_SSIZE_T      = Types::SSIZE_T
  TYPE_SIZE_T       = Types::SIZE_T
  TYPE_PTRDIFF_T    = Types::PTRDIFF_T
  TYPE_INTPTR_T     = Types::INTPTR_T
  TYPE_UINTPTR_T    = Types::UINTPTR_T
end
