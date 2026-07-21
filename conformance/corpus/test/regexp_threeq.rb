# Regexp#=== matches a string against the pattern (the operator
# `case x when /re/` uses). Previously fell through to the
# unresolved-call path and raised at runtime.
puts (/foo/ === "foobar")
puts (/foo/ === "bar")

case "hello"
when /h.*o/ then puts "matched"
else puts "no match"
end

case "world"
when /^h/ then puts "starts with h"
else puts "other"
end
