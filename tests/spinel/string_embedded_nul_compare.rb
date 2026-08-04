# A NUL is an ordinary byte in a Ruby String, and the representation records
# the real length -- but comparison went through strcmp, which stops at the
# first one. Two strings differing only after a NUL compared equal, so they
# also sorted equal, deduplicated into one, and collapsed as Hash keys. #3471,
# #3472.
s = "a\0b"
t = "a\0c"
p(s == t)
p(s.casecmp(t))
p(s.casecmp?(t))
p([t, s].sort.map(&:bytes))
p([s, t].max.bytes)
p([s, t].min.bytes)
p([s, t].uniq.size)
p([s, t].count(t))
p([s, t].index(t))
p([s, t].include?(t))
p(([s, t] - [s]).map(&:bytes))
p(([s, t] & [t]).map(&:bytes))
p(([s] | [t]).map(&:bytes))
p([s, t].group_by { |x| x }.size)
p({ s => 1, t => 2 }.size)
# The comparison operators themselves went through strcmp too, so `<=>` said 0
# where `==` said false, and the relational four ordered by the shared prefix.
p(s <=> t)
p(s < t)
p(s > t)
p(s <= t)
p(s >= t)
p(s.between?("a\0a", "a\0d"))
p(s.clamp("a\0c", "a\0d").bytes)
p((s.."a\0d").include?(t))
u = "a" + 0.chr + "b"
p(u <=> "a\0c")
p(u < "a\0c")
case t
when s.."a\0d" then puts "in"
else puts "out"
end
