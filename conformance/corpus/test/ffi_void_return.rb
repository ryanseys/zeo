module LibC
  ffi_func :malloc, [:size_t], :ptr
  ffi_func :free,   [:ptr],    :void
end

# void-returning functions can be used in any expression position;
# the call evaluates and the value is dropped.
p = LibC.malloc(8)
LibC.free(p)
puts "freed"

# As an expression: void-returning calls produce 0.
x = (LibC.free(LibC.malloc(8)); 42)
puts x
