# The time library is linked: parsing, arithmetic and formatting all work
# without a system ruby.
t = Time.at(1_700_000_000).utc
puts t.strftime("%Y-%m-%d %H:%M:%S %Z")
puts (t + 86_400).strftime("%A")
puts t.to_i, t.usec
puts Time.utc(2026, 9, 2).yday
__END__
2023-11-14 22:13:20 UTC
Wednesday
1700000000
0
245
