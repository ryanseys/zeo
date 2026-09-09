# `Time.at(1_600_000_000.5).utc.to_f` keeps the half second.
# (spinel issue #2865)
c = Time.at(1_600_000_000.5)
p c.utc.to_f
__END__
1600000000.5
