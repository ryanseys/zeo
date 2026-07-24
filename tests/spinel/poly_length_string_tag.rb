# A poly receiver carrying a string at runtime previously had no
# .length / .size dispatch arm — the per-tag emit covered SP_TAG_OBJ
# (Array variants) but not SP_TAG_STR. Result: `v.length` on a
# string-tagged poly silently returned 0. Surfaces when an
# is_a?(String) guard appears alongside a length check:
#   if v.is_a?(String) && v.length > 0; ... end
# The && condition's right half always read 0 and the then-arm
# never fired.

class C; def x; 1; end; end

arr = ["hello", C.new]
v = arr[0]
if v.is_a?(String) && v.length > 0
  puts v
end
if v.is_a?(String)
  puts v.size.to_s
end
puts "done"
