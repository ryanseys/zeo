# A NAMELESS `enum [...]` registers no type name and is consumed; a class
# that `extend FFI::DataConverter` with a `native_type` stands for that
# native type in later declarations (google-protobuf's Internal::Arena).

require "ffi"
module Internal
  class Arena
    extend ::FFI::DataConverter
    native_type ::FFI::Type::POINTER
  end
end
module L
  extend FFI::Library
  ffi_lib FFI::CURRENT_PROCESS
  enum [:small, :medium, :large]
  attach_function :my_memchr, :memchr, [Internal::Arena, :int, :size_t], :pointer
end
puts "compiled"
__END__
compiled
