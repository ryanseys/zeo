# A bigint value stored in a heterogeneous (poly) Array or Hash used to box as
# nil, because the poly value (sp_RbVal) had no bigint tag. With SP_TAG_BIGINT it
# is a first-class poly element: display, ==, include?, and hash-key all work.
# (b is promoted to bigint by the self-referential multiply in the while loop,
# so this holds in every overflow mode.)
b = 1
i = 0
while i < 70
  b = b * 2
  i = i + 1
end
b2 = 1
j = 0
while j < 70
  b2 = b2 * 2
  j = j + 1
end

arr = [1, "x", b]
p arr
puts arr.include?(b2)        # equality across the boxed bigint
puts arr[2] == b2

h = { small: 5, big: b }
p h
puts h[:big] == b2

# bigint as a hash key
hk = {}
hk[b] = "found"
p hk[b2]
