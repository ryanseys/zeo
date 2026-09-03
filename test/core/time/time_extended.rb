# Time#asctime/to_a/to_r/round/floor/ceil/xmlschema/deconstruct_keys.
# All in UTC so the golden output is portable across build hosts.
t = Time.at(1_700_000_000.5).utc
puts t.asctime
p t.to_a
p t.to_r
p Time.at(100).to_r                 # always Rational, even whole: (100/1)
p t.round.subsec                    # 0
p Time.at(1_700_000_000.7654321).round(3).subsec
p Time.at(1_700_000_000.7654321).floor(2).subsec
p Time.at(1_700_000_000.7654321).ceil(2).subsec
p Time.at(-0.5).round.to_r          # half-up toward +Infinity => (0/1)
puts t.xmlschema
puts t.xmlschema(3)
p t.deconstruct_keys([:year, :month, :day])
p t.deconstruct_keys(nil)
__END__
Tue Nov 14 22:13:20 2023
[20, 13, 22, 14, 11, 2023, 2, 318, false, "UTC"]
(3400000001/2)
(100/1)
0
(153/200)
(19/25)
(77/100)
(0/1)
2023-11-14T22:13:20Z
2023-11-14T22:13:20.500Z
{year: 2023, month: 11, day: 14}
{year: 2023, month: 11, day: 14, yday: 318, wday: 2, hour: 22, min: 13, sec: 20, subsec: (1/2), dst: false, zone: "UTC"}
