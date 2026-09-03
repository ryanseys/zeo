# `Time.parse` and its stricter siblings, from the vendored `time` gem. All of
# them delegate to `Date._parse` (see `tests/date_parse_heuristics.rb`); this
# file pins what the gem builds on top of it.
#
# Every case names its zone explicitly and compares in UTC: a string with no
# zone means LOCAL time, which is not the same answer on two machines.
require "time"

p Time.parse("2024-01-15 10:30:00 UTC").utc.to_s
p Time.parse("2024-01-15T10:30:00Z").utc.to_s
p Time.parse("2024-01-15T10:30:00+09:00").utc.to_s
p Time.parse("2024-01-15T10:30:00-05:00").utc.to_s
p Time.parse("Mon, 15 Jan 2024 10:30:00 +0900").utc.to_s
p Time.parse("Fri, 15 Jan 2024 10:30:00 GMT").utc.to_s
p Time.parse("15 Jan 2024 10:30:00 UTC").utc.to_s
p Time.parse("2024-01-15T10:30:00.5Z").utc.usec

# The format-specific parsers, which validate rather than guess.
p Time.iso8601("2024-01-15T10:30:00Z").utc.to_s
p Time.xmlschema("2024-01-15T10:30:00+09:00").utc.to_s
p Time.rfc2822("Mon, 15 Jan 2024 10:30:00 +0900").utc.to_s
p Time.rfc822("Mon, 15 Jan 2024 10:30:00 -0500").utc.to_s
p Time.httpdate("Fri, 15 Jan 2024 10:30:00 GMT").utc.to_s

# And the formatters that answer them.
t = Time.utc(2024, 1, 15, 10, 30, 0)
p t.iso8601
p t.xmlschema
p t.rfc2822
p t.httpdate

# A round trip through each format.
p Time.iso8601(t.iso8601).utc == t
p Time.rfc2822(t.rfc2822).utc == t
p Time.httpdate(t.httpdate).utc == t

# A string with no time in it at all, and one the strict parsers reject.
begin
  Time.parse("garbage")
rescue ArgumentError => e
  p [:parse, e.message]
end
begin
  Time.rfc2822("2024-01-15")
rescue ArgumentError => e
  p [:rfc2822, e.message]
end
begin
  Time.httpdate("Mon Jan 15 2024")
rescue ArgumentError => e
  p [:httpdate, e.message]
end

# `Time.zone_offset` reads the same zone names the scanner does.
p Time.zone_offset("UTC")
p Time.zone_offset("EST")
p Time.zone_offset("+09:00")
p Time.zone_offset("-0530")
p Time.zone_offset("nowhere")
__END__
"2024-01-15 10:30:00 UTC"
"2024-01-15 10:30:00 UTC"
"2024-01-15 01:30:00 UTC"
"2024-01-15 15:30:00 UTC"
"2024-01-15 01:30:00 UTC"
"2024-01-15 10:30:00 UTC"
"2024-01-15 10:30:00 UTC"
500000
"2024-01-15 10:30:00 UTC"
"2024-01-15 01:30:00 UTC"
"2024-01-15 01:30:00 UTC"
"2024-01-15 15:30:00 UTC"
"2024-01-15 10:30:00 UTC"
"2024-01-15T10:30:00Z"
"2024-01-15T10:30:00Z"
"Mon, 15 Jan 2024 10:30:00 -0000"
"Mon, 15 Jan 2024 10:30:00 GMT"
true
true
true
[:parse, "no time information in \"garbage\""]
[:rfc2822, "not RFC 2822 compliant date: \"2024-01-15\""]
[:httpdate, "not RFC 2616 compliant date: \"Mon Jan 15 2024\""]
0
-18000
32400
-19800
nil
