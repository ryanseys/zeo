module Flags
  ffi_const :READ,  1
  ffi_const :WRITE, 2
  ffi_const :EXEC,  4
  ffi_const :MASK,  0xff
end

puts Flags::READ
puts Flags::WRITE
puts Flags::EXEC
puts Flags::MASK
puts(Flags::READ | Flags::WRITE | Flags::EXEC)
