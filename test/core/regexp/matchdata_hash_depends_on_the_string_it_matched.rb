# Two matches of one pattern against different strings hash differently; the
# same string twice hashes alike.
# (spinel issue #3014)
m1 = "abc".match(/b/)
m2 = "xbz".match(/b/)
m3 = "abc".match(/b/)
p(m1.hash == m2.hash)
p(m1.hash == m3.hash)
p(m1.hash.is_a?(Integer))
__END__
false
true
true
