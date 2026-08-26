# `Array#join` on a self-referential array raises ArgumentError
# ("recursive array join"). It used to abort the process: the receiver's
# lock was still held when the nested join re-entered it, which is a
# re-entrant container access, and in a release build that aliases rather
# than asserting.
#
# The guard is only half the fix. Hoisting the payload clone OUT of the
# recursive call's argument list is the other half, and the SOUNDNESS half
# -- the caller-side rule the whole bug class comes down to is: never hold
# a `.lock()` inside a call-argument expression that also contains the
# recursive call.

a = [1]
a << a
begin
  a.join(",")
rescue ArgumentError => e
  puts e.message
end

# Nested one level deeper, so the guard has to be a STACK and not a flag
# on the receiver.
b = [1]
b << [2, b]
begin
  b.join(",")
rescue ArgumentError => e
  puts e.message
end

# A sibling is not a cycle: the same array twice is legal, and stays legal.
# This is what a monotonic visited-set would break.
c = [1, 2]
p [c, c].join(",")
p [c, [c, c]].join("-")

# Every ordinary join still works, including the nested and separatorless
# forms.
p [1, [2, [3, 4]], 5].join("-")
p [1, [2]].join
p [].join(",")
p [[], [[]]].join(",")
