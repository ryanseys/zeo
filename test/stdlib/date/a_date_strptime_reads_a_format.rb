# `Date._strptime` is the format-DIRECTED scanner behind `Date.strptime`,
# `DateTime.strptime` and the `time` gem's `Time.strptime`. aws-sigv4's
# `presign_url` reaches it through the last of those.
#
# The answered hash's key order IS the order the directives matched, so these
# are compared as printed.
require "date"
require "time"

p Date._strptime("2000-10-31", "%Y-%m-%d")
p Date._strptime("2013-05-24T00:00:00Z", "%Y-%m-%dT%H:%M:%S%Z")

# `%Y` is unbounded, EXCEPT when digits follow it in the format -- otherwise it
# swallows the month and day too (CRuby's `NUM_PATTERN_P`).
p Date._strptime("20130524T000000Z", "%Y%m%dT%H%M%S%Z")
p Date._strptime("2013-05-24", "%Y-%m-%d")

# A format the input does not match is nil, not a partial answer.
p Date._strptime("nope", "%Y-%m-%d")
p Date._strptime("2000-13-31", "%Y-%m-%d")
p Date._strptime("", "%Y")

# Input left OVER is fine and is reported.
p Date._strptime("2000-10-31 extra", "%Y-%m-%d")

# Fractions are exact Rationals, not floats.
p Date._strptime("2024-01-02 03:04:05.678 +0530", "%Y-%m-%d %H:%M:%S.%L %z")
p Date._strptime("01.123456789", "%S.%N")

p Date._strptime("1369353600", "%s")
p Date._strptime("24/12/99", "%d/%m/%y")
p Date._strptime("24/12/68", "%d/%m/%y")
p Date._strptime("15 Jan 2024", "%d %b %Y")
p Date._strptime("15 January 2024", "%d %B %Y")
p Date._strptime("Tuesday", "%A")
p Date._strptime("07:30 PM", "%I:%M %p")
p Date._strptime("07:30 am", "%I:%M %p")
p Date._strptime("2024-01-02", "%F")
p Date._strptime("03:04:05", "%T")
p Date._strptime("50%", "%d%%")

# What the gems actually call. `DateTime.strptime` is deliberately absent
# here: zeo's DateTime does not model sub-day fields at all, which predates
# this scanner -- see tests/gaps/a_datetime_keeps_its_time_of_day.rb.
p Date.strptime("2000-10-31", "%Y-%m-%d").to_s
p Time.strptime("2013-05-24T00:00:00Z", "%Y-%m-%dT%H:%M:%S%Z").utc.iso8601
__END__
{year: 2000, mon: 10, mday: 31}
{year: 2013, mon: 5, mday: 24, hour: 0, min: 0, sec: 0, zone: "Z", offset: 0}
{year: 2013, mon: 5, mday: 24, hour: 0, min: 0, sec: 0, zone: "Z", offset: 0}
{year: 2013, mon: 5, mday: 24}
nil
nil
nil
{year: 2000, mon: 10, mday: 31, leftover: " extra"}
{year: 2024, mon: 1, mday: 2, hour: 3, min: 4, sec: 5, sec_fraction: (339/500), zone: "+0530", offset: 19800}
{sec: 1, sec_fraction: (123456789/1000000000)}
{seconds: 1369353600}
{mday: 24, mon: 12, year: 1999}
{mday: 24, mon: 12, year: 2068}
{mday: 15, mon: 1, year: 2024}
{mday: 15, mon: 1, year: 2024}
{wday: 2}
{hour: 19, min: 30}
{hour: 7, min: 30}
{year: 2024, mon: 1, mday: 2}
{hour: 3, min: 4, sec: 5}
nil
"2000-10-31"
"2013-05-24T00:00:00Z"
