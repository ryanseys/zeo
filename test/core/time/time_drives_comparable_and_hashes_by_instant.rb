# Time includes Comparable, so the whole ordering surface falls out of
# `<=>`. Equality is by INSTANT, so a Time equals its own UTC copy and they
# hash alike (which is what makes a Time usable as a Hash key).

p Time.at(5) < Time.at(6)
p Time.at(6) > Time.at(5)
p Time.at(5).between?(Time.at(1), Time.at(9))
p Time.at(1).clamp(Time.at(2), Time.at(5)).to_i
p [Time.at(3), Time.at(1), Time.at(2)].sort.map(&:to_i)
p Time.at(5) == Time.at(5)
p Time.at(5).getutc == Time.at(5)
p Time.at(1.5).hash == Time.at(1.5).hash
p({ Time.at(99) => "found" }[Time.at(99)])
p (Time.at(5) <=> Time.at(6))
p (Time.at(5) <=> 5)
__END__
true
true
true
2
[1, 2, 3]
true
true
true
"found"
-1
nil
