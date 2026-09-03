# `min(n)` counts UP from begin and `max(n)` counts DOWN from end, so a range
# open on the OTHER side still answers. Sorting the whole range instead, as the
# generic Enumerable path does, never returns on an endless one.
#
# A Float begin cannot be walked, which is where the TypeErrors come from.
p((1..).min(3))
p((..5).max(2))
p(("a".."e").min(2))
p(("a".."e").max(2))
r001 = ((1.0..5.0).min(2) rescue $!.class); p r001
r002 = ((1.0..5.0).max(2) rescue $!.class); p r002
p((1..5.0).min(2))
p((1..5).min(2))
p((1..5).max(2))
__END__
[1, 2, 3]
[5, 4]
["a", "b"]
["e", "d"]
TypeError
TypeError
[1, 2]
[1, 2]
[5, 4]
