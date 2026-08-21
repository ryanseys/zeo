# A format's `%<name>` / `%{name}` is scanned into a C stack buffer and then
# compared against the hash's keys. The comparison read a marker byte in FRONT
# of both operands to recover a length that can carry an embedded NUL -- and a
# stack buffer has no marker, so it read whatever preceded it on the stack.
# When that byte happened to look like a marker it read a bogus header and
# answered a garbage length, and the lookup missed:
#
#   key<n> not found (KeyError)
#
# Nondeterministic by construction, which is what made this test flaky.
printf("%<n>04d\n", n: 7)
printf("%{k}\n", k: 3)
printf("%<x>s|%{y}\n", x: "a", y: "b")
p(format("%<v>d", v: 42))
p(format("%{one}-%{two}", one: "a", two: "b"))
p(format("%<a>s%<b>s%<c>s", a: 1, b: 2, c: 3))

# a name that is not there is CRuby's KeyError, worded for the spelling used
# -- and raising out of the lookup must not leak the caller's format buffer
begin
  format("%<missing>d", other: 1)
rescue KeyError => e
  p [e.class, e.message]
end
begin
  format("%{missing}", other: 1)
rescue KeyError => e
  p e.message
end
50.times do
  begin
    format("%{gone}", other: 1)
  rescue KeyError
  end
end
puts "no leak"

# repeated in a loop, where the stack under the buffer keeps changing
20.times { |i| print format("%<i>02d,", i: i) }
puts
