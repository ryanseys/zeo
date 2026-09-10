# MatchData#deconstruct_keys raises TypeError for an argument that is
# neither nil nor an Array of Symbols.
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
