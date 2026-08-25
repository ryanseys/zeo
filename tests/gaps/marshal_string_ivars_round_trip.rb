# Instance variables on a STRING survive a Marshal round-trip (the I
# prefix carries them); zeo drops them silently -- object ivars already
# round-trip. Data loss, not a missing error. (Found by the 2026-08-24
# probe sweep.)
s = +"payload"
s.instance_variable_set(:@tag, :iv)
r = Marshal.load(Marshal.dump(s))
p r
p r.instance_variable_get(:@tag)
