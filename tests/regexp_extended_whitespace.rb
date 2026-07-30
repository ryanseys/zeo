# Under `/x` the `regex` crate strips whitespace INSIDE a character class as
# well as outside it -- unlike Ruby, which only ignores it between tokens. That
# matters because zeo expands `\s` to a class, so a literal space written as one
# of its members disappeared and `/\s/x` stopped matching a space.
p(/\s/x =~ " ")
p(/\S/x =~ "a")
p(/a\s+b/x =~ "a b")
p(/\A\s*(\d{1,2})/x =~ " 15")
p($1)
p(/\s/x.match?("\t"))
p(/\s/x.match?("\n"))
p(/\s/x.match?("\v"))
p(/\s/x.match?("x"))

# Written inside a class, and negated there.
p(/[\s]/x =~ " ")
p(/[^\s]/x =~ " a")
p(/[\sx]/x =~ " ")

# The other shorthand classes expand the same way and were never affected --
# they carry no literal whitespace.
p(/\d/x =~ "7")
p(/\w/x =~ "_")
p(/\h/x =~ "f")

# Without `/x` nothing changed.
p(/\s/ =~ " ")
p(/[\s]/ =~ " ")

# The shape this was found in: an RFC 2822 date, whose pattern is written
# across several lines and so must be `/x`.
re = /\A\s*
      (?:(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)\s*,\s*)?
      (\d{1,2})\s+
      (Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)\s+
      (\d{2,})\s+
      (\d{2})\s*
      :\s*(\d{2})
      (?:\s*:\s*(\d\d))?\s+
      ([+-]\d{4}|UT|GMT|EST|EDT|CST|CDT|MST|MDT|PST|PDT|[A-IK-Z])/ix
p(re =~ "Mon, 15 Jan 2024 10:30:00 +0900")
p [$1, $2, $3, $4, $5, $6, $7]
p(re =~ "5 Feb 24 09:08 GMT")
p [$1, $2, $3, $4, $5, $6, $7]
p(re =~ "not a date")

# A `#` comment is still a comment under `/x`, and an ESCAPED space still
# matches one.
p(/a # trailing comment
  b/x =~ "ab")
p(/a\ b/x =~ "a b")
