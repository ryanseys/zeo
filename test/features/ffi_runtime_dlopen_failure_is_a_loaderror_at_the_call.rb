# Every candidate failing to open is the gem's own `LoadError`, raised at
# the CALL -- a binary whose optional native half is absent still starts.

require "ffi"
module M
  extend FFI::Library
  ffi_lib "#{'/no'}/such/dir/libnothing.so"
  attach_function :nope, [], :int
end
puts "started"
begin
  M.nope
rescue LoadError => e
  puts e.message.start_with?("Could not open library")
end
__END__
started
true
