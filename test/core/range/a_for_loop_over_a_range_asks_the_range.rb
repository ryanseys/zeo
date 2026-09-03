# `for x in <range>` decides how to iterate from the range's ENDPOINTS, not
# from the fact that it is a Range.
#
# Every shape below used to end the process with
#   thread 'ruby-main' panicked: expected an Int, got c
# because the lowering read both endpoints with `as_int_unchecked`. A String
# range yields Strings, a Float or beginless one yields nothing (ruby raises a
# catchable TypeError from `Range#each`), and an ENDLESS one has no end to
# read at all.
#
# The last case is the subtle one: a range reaching the loop through a local
# whose type is not statically Range still must not be COLLECTED, or an
# endless one never finishes.

for s in ("a".."c")
  puts s
end

r = ("x".."z")
for t in r
  puts t
end

begin
  for f in (1.0..3.0)
    puts f
  end
rescue TypeError => e
  puts "float: #{e.message}"
end

begin
  for b in (..5)
    puts b
  end
rescue TypeError => e
  puts "beginless: #{e.message}"
end

# Endless, three ways in: a literal, a plainly Range-typed local, and a local
# the compiler cannot call a Range (it held an exception first).
for i in (1..)
  break if i > 2
end
puts "endless literal ok"

e2 = (7..)
n = 0
for j in e2
  n += j
  break if j >= 9
end
p n

begin
  raise TypeError, "widen"
rescue TypeError => w
  w = (4..)
  m = 0
  for k in w
    m += k
    break if k >= 6
  end
  p m
end

# Bounded ranges are unchanged, literal and through variables, inclusive and
# exclusive, and an empty one runs its body zero times.
out = []
for a in (1..3) do out << a end
p out
lo = 2
hi = 5
out = []
for c in (lo...hi) do out << c end
p out
out = []
p(for d in (3..1) do out << d end)
p out
__END__
a
b
c
x
y
z
float: can't iterate from Float
beginless: can't iterate from NilClass
endless literal ok
24
15
[1, 2, 3]
[2, 3, 4]
3..1
[]
