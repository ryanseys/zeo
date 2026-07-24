# String#dump: a re-parseable literal; String#undump: its inverse.

# Named control escapes and quoting.
puts "hello".dump
puts "tab\tnewline\nreturn\r".dump
puts "bell\a back\b form\f vtab\v esc\e".dump
puts "quote\"q back\\slash".dump

# A NUL and other non-named control bytes become \xNN (uppercase).
puts "null\0byte".dump
puts "\x01\x02\x1f ctrl \x7f".dump

# '#' is escaped only before an interpolation sigil.
puts ("hash#" + "{x}").dump
puts ("hash#" + "$g").dump
puts ("hash#" + "@v").dump
puts "hash#plain".dump

# UTF-8 non-ASCII uses \u escapes (BMP as \uXXXX, astral as \u{...}).
puts "café".dump
puts "emoji😀".dump

# A binary string uses \xNN for high bytes and keeps its encoding.
puts "\xC3\xA9".b.dump

# undump is the exact inverse.
p "café".dump.undump
p "café".dump.undump == "café"
p "\x01\x02\x1f\x7f ctrl".dump.undump.bytes
p "emoji😀".dump.undump
p "\xC3\xA9".b.dump.undump == "\xC3\xA9".b

# undump rejects malformed input.
begin
  "not a dump".undump
rescue RuntimeError => e
  puts "raised: #{e.message}"
end
begin
  "café".undump
rescue RuntimeError => e
  puts "raised: #{e.message}"
end
