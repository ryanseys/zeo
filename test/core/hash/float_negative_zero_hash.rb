# GAP -- imported from the spinel corpus at c55d9bdb.
# -0.0 and 0.0 hash differently from ruby's answer.
#
p((-0.0).eql?(0.0))
p((-0.0).hash == (0.0).hash)
v001 = -0.0; w001 = 0.0; c001 = (v001.hash == w001.hash); p c001
h001 = { 0.0 => "a" }; p h001[-0.0]
p(-0.0 == 0.0)
__END__
true
true
true
"a"
true
