# Ruby warns when a conditional's test is an assignment of a LITERAL --
# `warning: found '= literal' in conditional, should be ==` -- the classic
# `if x = 1` typo. zeo emits nothing.
#
# The warning is deliberately narrow: only a literal right-hand side triggers
# it, because `if (m = gets)` is an idiom and must stay quiet. So the cases it
# does fire on are almost always the mistake it is named for, which is what
# makes it worth having.
#
# zeo already emits parse warnings from `zeo_rt::emit_parse_warnings`, so the
# channel exists; this diagnostic is not among the ones it carries. Both
# streams below are compared, and the warning belongs on stderr.

x = 0
if x = 1
  puts "literal assign taken"
end

while x = 2
  break
end

puts "unless:" if (x = 3)

# No warning for a non-literal right-hand side -- these must stay silent.
def gets_line = "value"
if (m = gets_line)
  puts "idiom ok: #{m}"
end
y = [1]
if (z = y.first)
  puts "call ok: #{z}"
end

puts "done"
