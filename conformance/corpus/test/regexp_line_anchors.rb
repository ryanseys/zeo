# Ruby's ^ and $ are always line-anchored, independent of the /m flag
# (which in Ruby only makes `.` match newlines). ^ matches at the start
# of the string and after any newline; $ matches at the end and before
# any newline. \A / \z / \Z are the absolute-boundary anchors.

# $ matches before a trailing newline (the each_line + capture pattern).
m = "FontName Times\n".match(/^(\w+) (.+)$/)
p m[1]
p m[2]

# ^ and $ at interior line boundaries.
p "a\nb\nc".scan(/^\w/)
p "a\nb\nc".scan(/\w$/)
p "foo 1\nbar 2".scan(/^(\w+) (\d+)$/)

# Building a lookup table from "key value\n" lines with a $-anchored regex.
table = {}
"Name Ada\nLang Ruby\nYear 1995".each_line do |line|
  if (m = line.match(/^(\w+) (.+)$/))
    table[m[1]] = m[2]
  end
end
p table.keys
p table["Name"]
p table["Lang"]
p table["Year"]

# Absolute anchors are NOT line-anchored.
p "abc\n".match?(/c$/)
p "abc".match?(/b$/)
p "a\nb".match?(/\Ab/)
p "a\n".match?(/a\z/)
p "a\n".match?(/a\Z/)
p "a".match?(/a\z/)
