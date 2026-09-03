# GAP -- imported from the spinel corpus at c55d9bdb.
# MatchData#deconstruct_keys with a bad argument type answers {} where ruby
# raises TypeError.
#
m001 = "ab".match(/(?<a>a)/)
r001 = (m001.deconstruct_keys(["a"]) rescue $!.class); p r001
r002 = (m001.deconstruct_keys(1) rescue $!.class); p r002
p m001.deconstruct_keys(nil)
p m001.deconstruct_keys([:a])
p m001.deconstruct_keys([:a, :b])
__END__
TypeError
TypeError
{a: "a"}
{a: "a"}
{}
