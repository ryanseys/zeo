t = Time.utc(2001, 2, 3, 4, 5, 6)
puts t.getlocal(3600).utc_offset
puts t.getlocal(3600).hour
puts t.getlocal("+09:00").utc_offset
puts t.getlocal("+09:00").hour
puts t.getlocal("-05:00").utc_offset
puts t.getlocal(-18000).hour
l = t.localtime(7200)
puts l.utc_offset
puts l.hour
