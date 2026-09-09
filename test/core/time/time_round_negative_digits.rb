# From the spinel corpus (c55d9bdb).
# Time#round/#floor/#ceil with a negative digit count is CRuby's
# ArgumentError; the count was clamped to zero and the call answered a Time.
t001 = Time.utc(2020, 1, 1, 0, 0, 1, 123456)
r001 = (t001.round(-1) rescue $!.class); p r001
r002 = (t001.floor(-1) rescue $!.class); p r002
r003 = (t001.ceil(-1) rescue $!.class); p r003
p Time.utc(2020, 1, 1, 0, 0, 1, 123456).round(3).usec
p Time.utc(2020, 1, 1, 0, 0, 1, 123456).floor(3).usec
p Time.utc(2020, 1, 1, 0, 0, 1, 123456).ceil(3).usec
p Time.utc(2020, 1, 1, 0, 0, 1, 500000).round.sec
__END__
ArgumentError
ArgumentError
ArgumentError
123000
123000
124000
2
