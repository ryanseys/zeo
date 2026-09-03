# `utc`/`gmtime`/`localtime` convert the receiver IN PLACE and answer self;
# the `get*` forms answer a copy and leave the receiver alone.

t = Time.at(1700000000)
u = t.getutc
p u.utc?
p t.utc?          # getutc did NOT mutate t
t.utc
p t.utc?          # ...but utc did
puts t.to_s
__END__
true
false
true
2023-11-14 22:13:20 UTC
