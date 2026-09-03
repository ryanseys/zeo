# GAP -- imported from the spinel corpus at c55d9bdb.
# MatchData does not deconstruct for an array pattern in case/in.
#
m001 = "ab".match(/(a)(b)/)
p m001.deconstruct
case m001
in [x001, y001] then p [x001, y001]
else p :none
end
case m001
in {a: 1} then p :h
else p :noh
end
r1 = (case m001; in [x, y] then [x, y]; end rescue $!.class); p r1
__END__
["a", "b"]
["a", "b"]
:noh
["a", "b"]
