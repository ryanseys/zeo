# String#scrub replaces invalid UTF-8 bytes with the given
# replacement (or U+FFFD by default).
puts "\xff".scrub("?")
puts "a\xffb".scrub("?")
puts "valid".scrub("?")
# Default replacement (U+FFFD = "\xEF\xBF\xBD") is 3 bytes.
puts "\xff".scrub.bytes.inspect
