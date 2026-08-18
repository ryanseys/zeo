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
