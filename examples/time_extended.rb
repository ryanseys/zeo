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
