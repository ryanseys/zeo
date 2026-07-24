# Time instance accessors that read existing sp_Time fields:
#   utc? / gmt?      -> the is_utc presentation flag (bool)
#   tv_usec / usec   -> microsecond fraction (tv_nsec / 1000)
#   tv_nsec / nsec   -> nanosecond fraction
#   tv_sec           -> whole epoch seconds (alias of to_i)
g = Time.utc(1997, 11, 21, 9, 55, 6)
puts g.utc?
puts g.gmt?

l = Time.local(1997, 11, 21, 9, 55, 6)
puts l.utc?

a = Time.at(1.5)
puts a.tv_sec
puts a.tv_usec
puts a.usec
puts a.tv_nsec
puts a.nsec
puts a.utc?
