# time is vendored under gems/ now, but `Time.parse` still fails: it delegates
# to `Date._parse` (time.rb:383), which zeo native date extension does not
# implement.
require "time"
p Time.parse("2024-01-15 10:30:00 UTC").to_s
