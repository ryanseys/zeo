t = Time.at(1_700_000_000.5).utc
puts t.asctime
p t.to_a
p Time.at(100).to_r
p Time.at(1_700_000_000.7654321).round(3).subsec
puts t.xmlschema(3)
p t.deconstruct_keys([:year, :month])
__END__
Tue Nov 14 22:13:20 2023
[20, 13, 22, 14, 11, 2023, 2, 318, false, "UTC"]
(100/1)
(153/200)
2023-11-14T22:13:20.500Z
{year: 2023, month: 11}
