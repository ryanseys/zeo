t = Time.at(1234567890.123456789)
p t.floor(1).nsec
p t.floor(3).nsec
p t.ceil(3).nsec
p t.round(3).nsec
p t.round(0).nsec
p t.round(0).sec
p t.ceil(0).sec
p t.floor(2).class
u = Time.at(1000000000.9999999)
p u.ceil(1).nsec
p u.round(6).nsec
