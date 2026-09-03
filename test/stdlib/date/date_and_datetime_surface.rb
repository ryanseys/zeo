# The whole `Date`/`DateTime` surface, field by field, against ruby 4.0.6.
# Nothing here reads the machine's zone: `to_time` appears only through
# fields that a local Time renders identically everywhere.
require "date"

d  = Date.new(2024, 2, 29)
dt = DateTime.new(2024, 1, 2, 3, 4, 5, "+09:00")

FIELDS = %i[
  year month mon day mday wday yday jd mjd ld ajd amjd day_fraction
  cwday cweek cwyear leap? julian? gregorian? start infinite?
  to_s inspect iso8601 xmlschema rfc3339 rfc2822 httpdate jisx0301
  ctime asctime marshal_dump
  sunday? monday? tuesday? wednesday? thursday? friday? saturday?
].freeze

[d, dt].each do |x|
  FIELDS.each { |m| p [x.class.name, m, x.public_send(m)] }
end

# The clock half is PRIVATE on Date and public on DateTime.
%i[hour min minute sec second sec_fraction second_fraction offset zone].each do |m|
  p [:datetime, m, dt.public_send(m)]
  begin
    d.public_send(m)
    p [:date, m, :public]
  rescue NoMethodError
    p [:date, m, :not_public]
  end
end

# Conversions. `Date#to_datetime` keeps the SIMPLE representation, so its
# day fraction is the Integer zero rather than a Rational.
p d.to_date.to_s, d.to_datetime.to_s, d.to_datetime.day_fraction
p dt.to_date.to_s, dt.to_datetime.to_s
p dt.new_offset("+00:00").to_s, dt.new_offset(0).to_s
p dt.iso8601(3), dt.rfc3339(6), dt.jisx0301(3), dt.xmlschema(0)
p d.deconstruct_keys(nil)
p dt.deconstruct_keys(nil)

# Navigation and iteration.
p (d >> 1).to_s, (d << 1).to_s, (d >> 12).to_s
p Date.new(2024, 1, 31).next_month.to_s, Date.new(2024, 1, 31).next_month(2).to_s
p d.prev_year(3).to_s, d.next.to_s, d.succ.to_s
p d.step(Date.new(2024, 3, 10), 3).map(&:to_s)
p d.upto(Date.new(2024, 3, 2)).map(&:to_s)
p Date.new(2024, 3, 2).downto(d).map(&:to_s)
p d.step(Date.new(2024, 2, 25), -2).map(&:to_s)
p d.upto(Date.new(2024, 2, 25)).to_a

# Constructors and validators.
p Date.commercial(2024, 9, 4).to_s, Date.ordinal(2024, 60).to_s
p Date.commercial(2024, -1, -1).to_s, Date.ordinal(2024, -1).to_s
p Date.new(2024, -1, -1).to_s, Date.new(2024, 2, -1).to_s
p Date.valid_date?(2024, 2, 29), Date.valid_date?(2024, 2, 30)
p Date.valid_ordinal?(2024, 366), Date.valid_ordinal?(2024, 367)
p Date.valid_commercial?(2024, 9, 4), Date.valid_commercial?(2024, 53, 7)
p Date.valid_jd?(2460370)
p Date.leap?(2024), Date.gregorian_leap?(1900), Date.julian_leap?(1900)
p Date.jd(2460370).to_s
p Date.parse("2024-02-29").to_s, Date.strptime("29/02/2024", "%d/%m/%Y").to_s
p DateTime.parse("2024-01-02T03:04:05+09:00").to_s
p DateTime.strptime("2024-01-02 03:04:05", "%Y-%m-%d %H:%M:%S").to_s
p DateTime.jd(2460370, 3, 4, 5).to_s

# The calendar reform: `Date::ITALY` is the default start, so 1582-10-05
# through 1582-10-14 do not exist and earlier dates are Julian.
p Date.new(1582, 10, 4).to_s, Date.new(1582, 10, 15).to_s, Date.new(1, 1, 1).jd
p Date.jd(2299160).to_s, Date.jd(2299161).to_s
p d.england.to_s, d.italy.to_s, d.julian.to_s, d.julian.julian?
p d.new_start(Date::JULIAN).to_s, d.gregorian.to_s
p Date.new(1752, 9, 2, Date::ENGLAND).to_s

# Ordering, equality and hashing.
p (d <=> Date.new(2024, 3, 1)), (d <=> 2460370), (d <=> "x")
p (d == Date.new(2024, 2, 29)), (d === Date.new(2024, 2, 29)), (d === 2460370)
p d.eql?(Date.new(2024, 2, 29)), (d.hash == Date.new(2024, 2, 29).hash)
p [Date.new(2024, 3, 1), Date.new(2024, 1, 1)].sort.map(&:to_s)
p (d + 1).to_s, (d - 1).to_s, (d - Date.new(2024, 1, 1))
p (d + Rational(1, 2)).day_fraction, (d + 1.5).to_s

# strftime, including the directives only `date` has.
p d.strftime("%c %x %X %v %+ %r %R %T %D %F")
p d.strftime("%G %V %U %W %u %w %P %p %I %l %k %C %y %j")
p d.strftime("%H:%M:%S %z %:z %Z %s %Q %L %N")
p dt.strftime("%Y %H:%M:%S %z %:z %Z")

# Constants.
p Date::DAYNAMES, Date::ABBR_DAYNAMES
p Date::MONTHNAMES, Date::ABBR_MONTHNAMES
p Date::ITALY, Date::ENGLAND, Date::JULIAN, Date::GREGORIAN

# Errors.
begin; Date.new(2024, 13, 1); rescue => e; p [e.class.ancestors.include?(ArgumentError), e.message]; end
begin; Date.new("x"); rescue => e; p [e.class, e.message]; end
begin; Date.new(2024, 2, 29, "x"); rescue => e; p [e.class, e.message]; end
begin; d >> "x"; rescue => e; p [e.class, e.message]; end
begin; d + "x"; rescue => e; p [e.class, e.message]; end
begin; d - "x"; rescue => e; p [e.class, e.message]; end
__END__
["Date", :year, 2024]
["Date", :month, 2]
["Date", :mon, 2]
["Date", :day, 29]
["Date", :mday, 29]
["Date", :wday, 4]
["Date", :yday, 60]
["Date", :jd, 2460370]
["Date", :mjd, 60369]
["Date", :ld, 161210]
["Date", :ajd, (4920739/2)]
["Date", :amjd, (60369/1)]
["Date", :day_fraction, 0]
["Date", :cwday, 4]
["Date", :cweek, 9]
["Date", :cwyear, 2024]
["Date", :leap?, true]
["Date", :julian?, false]
["Date", :gregorian?, true]
["Date", :start, 2299161.0]
["Date", :infinite?, false]
["Date", :to_s, "2024-02-29"]
["Date", :inspect, "#<Date: 2024-02-29 ((2460370j,0s,0n),+0s,2299161j)>"]
["Date", :iso8601, "2024-02-29"]
["Date", :xmlschema, "2024-02-29"]
["Date", :rfc3339, "2024-02-29T00:00:00+00:00"]
["Date", :rfc2822, "Thu, 29 Feb 2024 00:00:00 +0000"]
["Date", :httpdate, "Thu, 29 Feb 2024 00:00:00 GMT"]
["Date", :jisx0301, "R06.02.29"]
["Date", :ctime, "Thu Feb 29 00:00:00 2024"]
["Date", :asctime, "Thu Feb 29 00:00:00 2024"]
["Date", :marshal_dump, [0, 2460370, 0, 0, 0, 2299161.0]]
["Date", :sunday?, false]
["Date", :monday?, false]
["Date", :tuesday?, false]
["Date", :wednesday?, false]
["Date", :thursday?, true]
["Date", :friday?, false]
["Date", :saturday?, false]
["DateTime", :year, 2024]
["DateTime", :month, 1]
["DateTime", :mon, 1]
["DateTime", :day, 2]
["DateTime", :mday, 2]
["DateTime", :wday, 2]
["DateTime", :yday, 2]
["DateTime", :jd, 2460312]
["DateTime", :mjd, 60311]
["DateTime", :ld, 161152]
["DateTime", :ajd, (42514178449/17280)]
["DateTime", :amjd, (1042169809/17280)]
["DateTime", :day_fraction, (2209/17280)]
["DateTime", :cwday, 2]
["DateTime", :cweek, 1]
["DateTime", :cwyear, 2024]
["DateTime", :leap?, true]
["DateTime", :julian?, false]
["DateTime", :gregorian?, true]
["DateTime", :start, 2299161.0]
["DateTime", :infinite?, false]
["DateTime", :to_s, "2024-01-02T03:04:05+09:00"]
["DateTime", :inspect, "#<DateTime: 2024-01-02T03:04:05+09:00 ((2460311j,65045s,0n),+32400s,2299161j)>"]
["DateTime", :iso8601, "2024-01-02T03:04:05+09:00"]
["DateTime", :xmlschema, "2024-01-02T03:04:05+09:00"]
["DateTime", :rfc3339, "2024-01-02T03:04:05+09:00"]
["DateTime", :rfc2822, "Tue, 2 Jan 2024 03:04:05 +0900"]
["DateTime", :httpdate, "Mon, 01 Jan 2024 18:04:05 GMT"]
["DateTime", :jisx0301, "R06.01.02T03:04:05+09:00"]
["DateTime", :ctime, "Tue Jan  2 03:04:05 2024"]
["DateTime", :asctime, "Tue Jan  2 03:04:05 2024"]
["DateTime", :marshal_dump, [0, 2460311, 65045, 0, 32400, 2299161.0]]
["DateTime", :sunday?, false]
["DateTime", :monday?, false]
["DateTime", :tuesday?, true]
["DateTime", :wednesday?, false]
["DateTime", :thursday?, false]
["DateTime", :friday?, false]
["DateTime", :saturday?, false]
[:datetime, :hour, 3]
[:date, :hour, :not_public]
[:datetime, :min, 4]
[:date, :min, :not_public]
[:datetime, :minute, 4]
[:date, :minute, :not_public]
[:datetime, :sec, 5]
[:date, :sec, :not_public]
[:datetime, :second, 5]
[:date, :second, :not_public]
[:datetime, :sec_fraction, (0/1)]
[:date, :sec_fraction, :not_public]
[:datetime, :second_fraction, (0/1)]
[:date, :second_fraction, :not_public]
[:datetime, :offset, (3/8)]
[:date, :offset, :not_public]
[:datetime, :zone, "+09:00"]
[:date, :zone, :not_public]
"2024-02-29"
"2024-02-29T00:00:00+00:00"
0
"2024-01-02"
"2024-01-02T03:04:05+09:00"
"2024-01-01T18:04:05+00:00"
"2024-01-01T18:04:05+00:00"
"2024-01-02T03:04:05.000+09:00"
"2024-01-02T03:04:05.000000+09:00"
"R06.01.02T03:04:05.000+09:00"
"2024-01-02T03:04:05+09:00"
{year: 2024, month: 2, day: 29, yday: 60, wday: 4}
{year: 2024, month: 1, day: 2, yday: 2, wday: 2, hour: 3, min: 4, sec: 5, sec_fraction: (0/1), zone: "+09:00"}
"2024-03-29"
"2024-01-29"
"2025-02-28"
"2024-02-29"
"2024-03-31"
"2021-02-28"
"2024-03-01"
"2024-03-01"
["2024-02-29", "2024-03-03", "2024-03-06", "2024-03-09"]
["2024-02-29", "2024-03-01", "2024-03-02"]
["2024-03-02", "2024-03-01", "2024-02-29"]
["2024-02-29", "2024-02-27", "2024-02-25"]
[]
"2024-02-29"
"2024-02-29"
"2024-12-29"
"2024-12-31"
"2024-12-31"
"2024-02-29"
true
false
true
false
true
false
true
true
false
true
"2024-02-29"
"2024-02-29"
"2024-02-29"
"2024-01-02T03:04:05+09:00"
"2024-01-02T03:04:05+00:00"
"2024-02-29T03:04:05+00:00"
"1582-10-04"
"1582-10-15"
1721424
"1582-10-04"
"1582-10-15"
"2024-02-29"
"2024-02-29"
"2024-02-16"
true
"2024-02-16"
"2024-02-29"
"1752-09-02"
-1
-1
nil
true
true
true
true
true
["2024-01-01", "2024-03-01"]
"2024-03-01"
"2024-02-28"
(59/1)
(1/2)
"2024-03-01"
"Thu Feb 29 00:00:00 2024 02/29/24 00:00:00 29-FEB-2024 Thu Feb 29 00:00:00 +00:00 2024 12:00:00 AM 00:00 00:00:00 02/29/24 2024-02-29"
"2024 09 08 09 4 4 am AM 12 12  0 20 24 060"
"00:00:00 +0000 +00:00 +00:00 1709164800 1709164800000 000 000000000"
"2024 03:04:05 +0900 +09:00 +09:00"
["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"]
["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
[nil, "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"]
[nil, "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"]
2299161
2361222
Infinity
-Infinity
[true, "invalid date"]
[TypeError, "invalid year (not numeric)"]
[TypeError, "no implicit conversion to float from string"]
[TypeError, "String can't be coerced into Integer"]
[TypeError, "expected numeric"]
[TypeError, "expected numeric"]
