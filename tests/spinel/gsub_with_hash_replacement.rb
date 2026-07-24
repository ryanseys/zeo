# #485. `String#gsub(regex, hash)` — the per-match hash-lookup form
# Ruby uses for HTML/JSON escape. Pre-fix codegen passed the hash
# pointer through to sp_re_gsub's `const char *rep` parameter; the
# binary read garbage from the pointer's memory in place of the
# replacement string. Fix: detect a str_str_hash second arg and
# route to a parallel runtime helper that does the per-match
# lookup.

ESCAPES = {
  "&" => "&amp;",
  "<" => "&lt;",
  ">" => "&gt;",
}.freeze

PATTERN = /[&<>]/.freeze

puts "a&b<c".gsub(PATTERN, ESCAPES)
puts "<p>x & y</p>".gsub(PATTERN, ESCAPES)
puts "no specials here".gsub(PATTERN, ESCAPES)
