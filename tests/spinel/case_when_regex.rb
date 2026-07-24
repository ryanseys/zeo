# Issue #852: `case <string> when /regex/` SEGV'd because the
# generic strcmp arm consumed the RegularExpressionNode's
# fallthrough value `0`. Route through sp_re_match_p.
case "hello"
when /^h/
  puts "starts with h"
when /world/
  puts "contains world"
end

case "world!"
when /^h/
  puts "starts with h"
when /world/
  puts "contains world"
end

case "abc"
when /xy/
  puts "match"
else
  puts "no match"
end
