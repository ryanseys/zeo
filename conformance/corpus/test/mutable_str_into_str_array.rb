# Pushing a mutable String builder (String.new + <<) into a str_array
# must snapshot its buffer into a sweep-owned (0xfe) copy, not store the
# sp_String pointer / alias its object-owned ->data buffer. The builder
# is otherwise unreferenced after the push, so a GC collects it and frees
# the buffer, leaving the array element dangling; the next mark walk
# faults scanning it (the #1071 / #1314 use-after-free class). Pre-fix
# this also emitted `sp_StrArray_push(arr, sp_String *)`, an
# incompatible-pointer-type the -Werror test build rejects outright.

# 1. `<<` (statement form) — many builders pushed under GC churn.
arr = []
i = 0
while i < 4000
  s = String.new
  s << "item "
  s << i.to_s
  arr << s
  i = i + 1
end
puts arr.length                 #=> 4000
puts arr[0]                     #=> item 0
puts arr[4000 - 1]             #=> item 3999

# 2. `push` (expression form).
arr2 = []
k = 0
while k < 2000
  b = String.new
  b << "k="
  b << k.to_s
  arr2.push(b)
  k = k + 1
end
puts arr2.length                #=> 2000
puts arr2[1000]                #=> k=1000

# 3. Sum the lengths back (reads every element after GC).
total = 0
j = 0
while j < arr.length
  total = total + arr[j].length
  j = j + 1
end
puts total                      #=> 34890

puts "done"
