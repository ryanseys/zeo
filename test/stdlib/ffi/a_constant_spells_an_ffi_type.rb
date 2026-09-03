# A constant holding a type symbol (`Word = :uint32`, smartcard) or an
# `FFI::Type::X` object (`CFIndex = FFI::Type::LONG_LONG`, audio) is FFI
# vocabulary: signatures and layouts naming the constant resolve through
# its value, exactly as ruby-ffi's find_type does. The platform-guarded
# spelling folds the same way at both stages.
require "ffi"

module Lib
  extend FFI::Library
  ffi_lib FFI::Library::LIBC

  if FFI::Platform.mac?
    Word = :uint32
  else
    Word = :uint32
  end
  # audio's spelling: the width picked by an ARCH string compare. Both
  # branches agree here so the golden holds on every host; the FOLD is
  # exercised either way (an unfolded guard poisons the constant).
  if FFI::Platform::ARCH == 'x86_64'
    CFIndex = FFI::Type::LONG_LONG
  else
    CFIndex = FFI::Type::LONG_LONG
  end

  attach_function :labs_w, :labs, [CFIndex], CFIndex

  class Holder < FFI::Struct
    layout :tag, Word,
           :count, CFIndex
  end
end

p Lib.labs_w(-42)
h = Lib::Holder.new
h[:tag] = 7
h[:count] = -3_000_000_000
p h[:tag]
p h[:count]
p Lib::Holder.size
puts "still running"
__END__
42
7
-3000000000
16
still running
