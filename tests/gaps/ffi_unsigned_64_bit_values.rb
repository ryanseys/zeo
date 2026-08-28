require "ffi"

m = FFI::MemoryPointer.new(:uint64, 1)
[0xFFFF_FFFF_FFFF_FFFF, 0x8000_0000_0000_0000, 1].each do |v|
  r = begin
    m.write_uint64(v)
    m.read_uint64
  rescue Exception => e
    "#{e.class}: #{e.message}"
  end
  puts "#{v}\t#{r}"
end

s = FFI::MemoryPointer.new(:int64, 1)
s.write_int64(-1)
p s.read_int64

u = FFI::MemoryPointer.new(:uint32, 1)
u.write_uint32(0xFFFF_FFFF)
p u.read_uint32
