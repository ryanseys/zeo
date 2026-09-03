# `Time#isdst`/`#dst?` report the broken-down time's DST flag; a UTC time is
# never in DST.

t = Time.at(0).utc
p t.isdst
p t.dst?
__END__
false
false
