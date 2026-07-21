module LibM
  ffi_func :cos,  [:double], :double
  ffi_func :sqrt, [:double], :double
  ffi_func :pow,  [:double, :double], :double
end

module LibC
  ffi_func :strlen, [:str], :size_t
  ffi_func :getpid, [],     :int
end

puts LibM.cos(0.0).to_i
puts LibM.sqrt(16.0).to_i
puts LibM.pow(2.0, 10.0).to_i
puts LibC.strlen("hello, world")
puts(LibC.getpid > 0 ? "pid_ok" : "pid_bad")
