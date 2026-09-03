# Kernel#warn(category:) honors CRuby's default warning levels: :deprecated is
# off by default (prints nothing), while :experimental and the uncategorized
# form print to stderr. The keyword Hash itself is never printed, and a
# suppressed warning's message arguments are still evaluated for side effects.
warn("plain warning")
warn("deprecated thing", category: :deprecated)
warn("experimental thing", category: :experimental)
warn("a", "b", category: :deprecated)
warn("c", "d", category: :experimental)

def shout
  $stdout.puts "evaluated"
  "msg"
end
warn(shout, category: :deprecated)

# String#to_i(0) auto-detects the base from the literal's prefix: 0x/0X hex,
# 0b/0B binary, 0o/0O or a bare leading 0 octal, 0d decimal, otherwise decimal.
puts "0xff".to_i(0)
puts "0b101".to_i(0)
puts "0o755".to_i(0)
puts "0777".to_i(0)
puts "0d42".to_i(0)
puts "42".to_i(0)
puts "-0xff".to_i(0)
puts "+0b101".to_i(0)
puts "  99 apples".to_i(0)
__END__
evaluated
255
5
493
511
42
42
-255
5
99
#@ stderr
plain warning
experimental thing
c
d
