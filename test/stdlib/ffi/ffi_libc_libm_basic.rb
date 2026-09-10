# Basic libc and libm bindings through the ffi gem: scalar doubles in and
# out, a string argument measured by strlen, and a no-arg int return.
require "ffi"

module LibM
  extend FFI::Library
  ffi_lib "m"
  attach_function :cos,  [:double], :double
  attach_function :sqrt, [:double], :double
  attach_function :pow,  [:double, :double], :double
end

module LibC
  extend FFI::Library
  ffi_lib FFI::Library::LIBC
  attach_function :strlen, [:string], :size_t
  attach_function :getpid, [],        :int
end

puts LibM.cos(0.0).to_i
puts LibM.sqrt(16.0).to_i
puts LibM.pow(2.0, 10.0).to_i
puts LibC.strlen("hello, world")
puts(LibC.getpid > 0 ? "pid_ok" : "pid_bad")
__END__
1
4
1024
12
pid_ok
