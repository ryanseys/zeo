# A Time read back out of a container is a boxed value, and the poly readers
# had no arm for it: to_i and to_f answered 0 and equality compared boxes.
a = [Time.utc(2020, 1, 2)]
t = a[0]
p t.year
p t.to_i
p t.to_f
p t == Time.utc(2020, 1, 2)
p t.eql?(Time.utc(2020, 1, 2))
p t == Time.utc(2021, 1, 2)

h = { k: Time.utc(2020, 1, 2) }
p h[:k].to_i
p h[:k].to_f
p h[:k] == Time.utc(2020, 1, 2)

# a Time with a fractional part keeps it through to_f
b = [Time.at(1, 500_000)]
p b[0].to_f
p b[0].to_i

# the unboxed forms are unchanged
u = Time.utc(2020, 1, 2)
p u.to_i
p u.to_f
p u == Time.utc(2020, 1, 2)
