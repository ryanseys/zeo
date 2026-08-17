# An index outside a MatchData's groups is CRuby's IndexError. begin, end and
# offset answered nil (and offset a pair of nils) instead.
m = "hello".match(/(l)/)
p((m.begin(5) rescue $!.class))
p((m.end(5) rescue $!.class))
p((m.offset(5) rescue $!.class))
p((m.begin(-1) rescue $!.message))
p m.begin(0)
p m.begin(1)
p m.end(1)
p m.offset(1)
p((m.bytebegin(9) rescue $!.class))
