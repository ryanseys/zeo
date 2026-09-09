# The lambda answers the rebound value, for one parameter, for two, and in a
# proc.
# (spinel issue #3309)
f = ->(s) { s = s[1..]; s }
p f.call("abc")
f2 = ->(a, b) { a = a + 1; b = b * 2; [a, b] }
p f2.call(1, 2)
pr = proc { |s| s = s.to_s; s }
p pr.call(5)
__END__
"bc"
[2, 4]
"5"
