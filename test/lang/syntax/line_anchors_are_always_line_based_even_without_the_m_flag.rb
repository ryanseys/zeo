# Real Ruby's `^`/`$` ALWAYS match at line boundaries -- there is no
# separate opt-in the way most other regex flavors need one. `/m`
# instead makes `.` match a newline too (verified against real `ruby`
# directly, since this is a common point of confusion between Ruby's
# flag vocabulary and most other engines').

puts("line1\nline2" =~ /^line2/)
s = "abc\ndef"
puts(s =~ /c.d/)
puts(s =~ /c.d/m)
__END__
6

2
