# `a.concat(a, a)` where an argument aliases the receiver, an escaped `#`
# before a brace in a pattern, and two more that once looped forever.

# Repro 1: self-concat with self twice.
a = [1, 2]
a.concat(a, a)
puts a.length      # 8 -- [1,2] + [1,2] + [1,2,1,2]

# Repro 2: regex with literal `\#{re}` characters.
re = /foo|bar/
r2 = /\#{re}/
puts "ok2"

# Repro 3: while () never enters.
while () ; 123 ; end
puts "ok3"

# Repro 4: retry-counter survives longjmp.
def foo
  i = 0
  begin ; i += 1 ; raise "bar" ; rescue ; retry unless i == 7 ; end
  raise "baz" unless i == 7
rescue
  123
end
puts foo  # 123  (foo's body would also raise "baz" if i != 7; here i == 7
          #       so no raise, and the function falls through. The
          #       outer rescue is dead code for this path.)
__END__
6
ok2
ok3

