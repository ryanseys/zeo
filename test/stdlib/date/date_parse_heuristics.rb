# `Date._parse` -- the heuristic scanner `Date.parse`, `DateTime.parse` and
# `Time.parse` all read their fields out of. Every line here is oracle-verified
# against ruby 4.0.6, including the KEY ORDER: the hash records fields in the
# order the cascade found them (weekday, then clock and zone, then the date),
# so the order is part of the answer.
#
# Known divergence: a string with no date in it raises ArgumentError here where
# ruby raises `Date::Error`, a subclass of it that zeo does not model (the date
# extension is native, with no Ruby half to declare the class in).
require "date"

# ISO 8601, in the shapes a timestamp actually arrives in.
p Date._parse("2024-01-15")
p Date._parse("2024-01-15T10:30:00Z")
p Date._parse("2024-01-15T10:30:00+09:00")
p Date._parse("2024-01-15T10:30:00.123456+09:00")
p Date._parse("2024-01-15 10:30:00 UTC")
p Date._parse("2024-01-15T10:30")
p Date._parse("2001-02-03T04:05:06.123456789")
p Date._parse("2024-01-15T10:30:00,5Z")

# RFC 2822 and HTTP-date, whose weekday name is recorded and then ignored.
p Date._parse("Mon, 15 Jan 2024 10:30:00 +0900")
p Date._parse("Fri, 15 Jan 2024 10:30:00 GMT")
p Date._parse("Sat, 03 Feb 2024 04:05:06 -0000")
p Date._parse("Thu Nov 29 14:33:20 2001")
p Date._parse("Wed, 15 May 2024")

# The month-NAME forms, in either order and with or without a year.
p Date._parse("15 Jan 2024")
p Date._parse("Jan 15 2024")
p Date._parse("January 15, 2024")
p Date._parse("15-Jan-2024")
p Date._parse("Aug 31")
p Date._parse("Aug 2000")
p Date._parse("Aug 3rd 2000")
p Date._parse("Jan '99")
p Date._parse("'99 Jan 15")
p Date._parse("Aug")

# Separator forms. A dot date reads year-first or day-first depending on which
# end carries the four-digit year.
p Date._parse("2024/01/15")
p Date._parse("2024.01.15")
p Date._parse("5.6.2024")
p Date._parse("31.12.99")
p Date._parse("7/23")
p Date._parse("2024/1")
p Date._parse("12/2024")
p Date._parse("1/2/3")

# A bare digit run is read by its LENGTH, with no validation at all.
p Date._parse("20240115")
p Date._parse("20240115T103000Z")
p Date._parse("20240115103000")
p Date._parse("2000")
p Date._parse("24")
p Date._parse("012")
p Date._parse("12345")
p Date._parse("1234567")
p Date._parse("3rd")
p Date._parse("15th")

# A clock on its own, with a meridian marker or a zone.
p Date._parse("10:30")
p Date._parse("10:30:00.5")
p Date._parse("12:00 pm")
p Date._parse("12:00 am")
p Date._parse("1:30pm")
p Date._parse("23:59:60")
p Date._parse("10:30 JST")
p Date._parse("10:30 -0530")
p Date._parse("10:30 gmt-3")
p Date._parse("10:30 Eastern Standard Time")
p Date._parse("10:30 EST5EDT")

# The other calendars ISO 8601 spells: a week date and an ordinal date.
p Date._parse("2024-W03-1")
p Date._parse("2024-015")

# A two-digit year widens only when the caller asked for a COMPLETE date.
p Date._parse("68-01-15")
p Date._parse("69-01-15")
p Date._parse("01-10-31")
p Date._parse("01-10-31", false)
p Date._parse("-4712-01-01")

# Nothing recognisable answers an empty hash rather than raising -- which is
# what lets `Time.parse` report its own error.
p Date._parse("")
p Date._parse("garbage")
p Date._parse("Sat")
p Date._parse("sunday")

# Commentary in parentheses is dropped, as RFC 2822 allows.
p Date._parse("2024-01-15 (a comment) 10:30")
p Date._parse("  2024-01-15  ")

# `Date.parse` completes the fields: everything ABOVE the most significant one
# given is unknown, and everything below it takes its minimum.
p Date.parse("2024-01-15").to_s
p Date.parse("2024-015").to_s
p Date.parse("2024-W03-1").to_s
p Date.parse("Aug 2000").to_s
p Date.parse("2024-01-15T10:30:00Z").to_s
p Date.parse("15-Jan-2024").to_s
begin
  Date.parse("10:30")
rescue ArgumentError => e
  p [:no_date, e.message]
end
begin
  Date.parse("2024")
rescue ArgumentError => e
  p [:not_a_day, e.message]
end

# The zone table `:offset` is filled from, exercised through the scanner (CRuby
# keeps `zone_to_diff` to itself, so there is no public row to call directly).
["+09:00", "-0530", "UTC", "PST", "CEST", "A", "Y", "nowhere"].each do |z|
  p [z, Date._parse("10:30 #{z}")[:offset]]
end
__END__
{year: 2024, mon: 1, mday: 15}
{zone: "Z", hour: 10, min: 30, sec: 0, year: 2024, mon: 1, mday: 15, offset: 0}
{zone: "+09:00", hour: 10, min: 30, sec: 0, year: 2024, mon: 1, mday: 15, offset: 32400}
{zone: "+09:00", hour: 10, min: 30, sec: 0, sec_fraction: (1929/15625), year: 2024, mon: 1, mday: 15, offset: 32400}
{zone: "UTC", hour: 10, min: 30, sec: 0, year: 2024, mon: 1, mday: 15, offset: 0}
{hour: 10, min: 30, year: 2024, mon: 1, mday: 15}
{hour: 4, min: 5, sec: 6, sec_fraction: (123456789/1000000000), year: 2001, mon: 2, mday: 3}
{zone: "Z", hour: 10, min: 30, sec: 0, sec_fraction: (1/2), year: 2024, mon: 1, mday: 15, offset: 0}
{wday: 1, zone: "+0900", hour: 10, min: 30, sec: 0, year: 2024, mon: 1, mday: 15, offset: 32400}
{wday: 5, zone: "GMT", hour: 10, min: 30, sec: 0, year: 2024, mon: 1, mday: 15, offset: 0}
{wday: 6, zone: "-0000", hour: 4, min: 5, sec: 6, year: 2024, mon: 2, mday: 3, offset: 0}
{wday: 4, hour: 14, min: 33, sec: 20, year: 2001, mon: 11, mday: 29}
{wday: 3, year: 2024, mon: 5, mday: 15}
{year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 1, mday: 15}
{mon: 8, mday: 31}
{year: 2000, mon: 8}
{year: 2000, mon: 8, mday: 3}
{year: 1999, mon: 1}
{year: 1999, mon: 1, mday: 15}
{mon: 8}
{year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 6, mday: 5}
{year: 2031, mon: 12, mday: 99}
{mon: 7, mday: 23}
{year: 2024, mon: 1}
{year: 2024, mon: 12}
{year: 2001, mon: 2, mday: 3}
{year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 1, mday: 15, hour: 10, min: 30, sec: 0, zone: "Z", offset: 0}
{year: 2024, mon: 1, mday: 15, hour: 10, min: 30, sec: 0}
{mon: 20, mday: 0}
{mday: 24}
{yday: 12}
{year: 2012, yday: 345}
{year: 1234, yday: 567}
{mday: 3}
{mday: 15}
{hour: 10, min: 30}
{hour: 10, min: 30, sec: 0, sec_fraction: (1/2)}
{hour: 12, min: 0}
{hour: 0, min: 0}
{hour: 13, min: 30}
{hour: 23, min: 59, sec: 60}
{zone: "JST", hour: 10, min: 30, offset: 32400}
{zone: "-0530", hour: 10, min: 30, offset: -19800}
{zone: "gmt-3", hour: 10, min: 30, offset: -10800}
{zone: "Eastern Standard Time", hour: 10, min: 30, offset: -18000}
{hour: 10, min: 30}
{cwyear: 2024, cweek: 3, cwday: 1}
{year: 2024, yday: 15}
{year: 2068, mon: 1, mday: 15}
{year: 1969, mon: 1, mday: 15}
{year: 2001, mon: 10, mday: 31}
{year: 1, mon: 10, mday: 31}
{year: -4712, mon: 1, mday: 1}
{}
{}
{wday: 6}
{wday: 0}
{hour: 10, min: 30, year: 2024, mon: 1, mday: 15}
{year: 2024, mon: 1, mday: 15}
"2024-01-15"
"2024-01-15"
"2024-01-15"
"2000-08-01"
"2024-01-15"
"2024-01-15"
[:no_date, "invalid date"]
[:not_a_day, "invalid date"]
["+09:00", 32400]
["-0530", -19800]
["UTC", 0]
["PST", -28800]
["CEST", 7200]
["A", 3600]
["Y", -43200]
["nowhere", nil]
