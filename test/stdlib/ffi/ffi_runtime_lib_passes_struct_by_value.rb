# A struct passed BY VALUE through the RUNTIME library tier (an ffi_lib only
# the running process can name): libffi builds the aggregate descriptor from
# the declaration's real field types, so the classification matches the
# extern tier's repr(C) mirror. libc's inet_ntoa takes struct in_addr by
# value; 0x0100007F little-endian is network-order 127.0.0.1.
require "ffi"

module Net
  extend FFI::Library
  lib = ENV["ZEO_TEST_NO_SUCH_ENV"] || "c"
  ffi_lib lib

  class InAddr < FFI::Struct
    layout :s_addr, :uint32
  end

  attach_function :inet_ntoa, [InAddr.by_value], :string
end

a = Net::InAddr.new
a[:s_addr] = 0x0100007F
puts Net.inet_ntoa(a)
a[:s_addr] = 0xFFFFFFFF
puts Net.inet_ntoa(a)
__END__
127.0.0.1
255.255.255.255
