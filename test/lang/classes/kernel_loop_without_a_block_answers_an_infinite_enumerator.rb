# It yields nil forever, so first, take and repeated next all answer.
# (spinel issue #3236)
e = loop
p e.class
p loop.first(3)
p loop.take(2)
e2 = loop
p e2.next
p e2.next
__END__
Enumerator
[nil, nil, nil]
[nil, nil]
nil
nil
