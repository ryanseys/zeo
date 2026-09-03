# Both loops splice into the enclosing scope and both first-assign `r`, so
# each gets storage of its own for its loop's extent. The two storages are
# registered under synthetic keys, and a key collision left the first loop's
# value with no release -- visible only under `ZEO_RT_LEAKCHECK`, which the
# memcheck leg runs over every golden.
1.times do |i|
  r = "a" + i.to_s
  p r
end
1.times do
  r = "c" + "d"
  p r
end

# The same pair with the parameter on the second loop instead.
2.times do
  s = "e" * 2
  p s
end
2.times do |i|
  s = "f" * i
  p s
end

# ...and with an accumulating kind between them, whose accumulator slot takes
# a synthetic key from the same counter.
p (1..3).map { |i| "m#{i}" }
1.times do |i|
  r = "g" + i.to_s
  p r
end
__END__
"a0"
"cd"
"ee"
"ee"
""
"f"
["m1", "m2", "m3"]
"g0"
