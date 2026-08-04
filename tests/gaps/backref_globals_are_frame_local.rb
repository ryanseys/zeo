# CRuby scopes $~ (and so $1/$&/$`/$') per method frame (the svar slot on the
# frame): a match inside a called method never touches the caller's backrefs.
# zeo stores one process-global last-match, so the callee's match leaks out.
# Second half: String#scan must SET $~ to its last match; zeo's scan leaves
# the global untouched.
"hello world" =~ /(w\w+)/
puts $1.inspect

def match_inside
  "abc" =~ /(b)/
  $1
end

puts match_inside.inspect
puts $1.inspect
puts $~[0].inspect

nums = "a1b2c3".scan(/\d/)
puts nums.inspect
puts $~[0].inspect

[1].each do
  "block shares the method frame" =~ /(shares)/
end
puts $1.inspect
