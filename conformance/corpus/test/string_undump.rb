# String#undump is the inverse of String#dump. Round-tripping a string
# through dump then undump returns the original (each engine round-trips
# its own dump format, so the comparison is version-independent).
# Previously undump was unresolved.
["hello", "a\tb\nc", "q\"u\\o#te", "tab\there", "café"].each do |s|
  puts(s.dump.undump == s)
end
# undump of an explicit dumped form
puts "\"hi\\nthere\"".undump
