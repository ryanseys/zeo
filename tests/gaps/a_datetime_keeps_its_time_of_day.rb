# `DateTime` models a calendar date only: zeo's `Date`/`DateTime` are backed by
# a Julian Day Number, and the module's own docs record the partial -- "its
# time-of-day fields default to midnight ... the calendar half is complete,
# sub-day fields are not modelled here".
#
# So every DateTime shape that carries a clock loses it. CRuby answers
# "2024-01-02T03:04:05+00:00"; zeo answers "2024-01-02".
#
# Pre-existing and independent of `Date._strptime`: `DateTime.parse` loses the
# time identically on the binary built before that scanner existed. Closing it
# means giving the backing object a day FRACTION beside its JDN, plus an
# offset, and teaching `to_s`/`hour`/`min`/`sec`/`zone` to read them.
require "date"

p DateTime.parse("2024-01-02T03:04:05+00:00").to_s
p DateTime.strptime("2024-01-02T03:04:05+00:00", "%Y-%m-%dT%H:%M:%S%z").to_s
p DateTime.new(2024, 1, 2, 3, 4, 5).to_s
p DateTime.parse("2024-01-02T03:04:05+00:00").hour
