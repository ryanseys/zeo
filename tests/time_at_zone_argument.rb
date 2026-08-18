p Time.at(0, in: "UTC").utc?
p Time.at(0, in: 3600).utc_offset
p Time.at(0, in: "Z").utc?
r = (Time.at(0, in: "bogus") rescue $!.class); p r
p Time.at(1, 2, in: "+09:00").to_i
p Time.at(0, in: "+09:00").utc_offset
p Time.at(Rational(3, 2), in: "+09:00").to_f
p Time.at(1.5, in: "+09:00").to_f
