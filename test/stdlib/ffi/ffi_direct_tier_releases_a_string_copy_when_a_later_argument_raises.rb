# A `:string` argument's NUL-terminated copy is owned by a pooled temp.
# When a LATER argument's coercion raises, the copy is released on the
# raise edge like any other temp -- the leak checker is what proves it,
# and the gem's own error texts are what the rescue prints.
#@ env: ZEO_RT_LEAKCHECK=1

require "ffi"
module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strtol, [:string, :pointer, :int], :long
  attach_function :my_strlen, :strlen, [:string], :ulong
end
3.times do
  begin
    LibC.strtol("42", nil, "ten")
  rescue TypeError => e
    puts e.message
  end
end
begin
  LibC.my_strlen("a\0b")
rescue ArgumentError => e
  puts e.message
end
puts LibC.strtol("42", nil, 10)
__END__
no implicit conversion of String into Integer
no implicit conversion of String into Integer
no implicit conversion of String into Integer
string contains null byte
42
