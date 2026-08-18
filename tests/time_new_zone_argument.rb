p Time.new(2020, 1, 1, 0, 0, 0, "UTC").utc?
p Time.new(2020, 1, 1, 0, 0, 0, "Z").utc?
p Time.new(2020, 1, 1, 0, 0, 0, "+0900").utc_offset
p Time.new(2020, 1, 1, 0, 0, 0, "+09:00:30").utc_offset
z = "+09:00"
p Time.new(2020, 1, 1, 0, 0, 0, z).utc_offset
r = (Time.new(2020, 1, 1, 0, 0, 0, "nope") rescue $!.class); p r
p Time.new(2020, 1, 1, 0, 0, 0, 3600).utc_offset
p Time.new(2020, 1, 1, in: "+02:00").utc_offset
