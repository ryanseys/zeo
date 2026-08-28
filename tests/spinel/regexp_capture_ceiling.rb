# A pattern wider than the match registers hold is refused rather than
# truncated. It used to compile and lose everything past the thirty-first
# group: `m.size` answered 32 where CRuby answers 41, and m[32] onward were
# nil, with nothing said about it. The ceiling and the registers agree now.
#
# CRuby has no such ceiling, so the refusals below are a deliberate divergence
# and this file does not read the same there; see docs/limitations.md. What is
# NOT a divergence is everything up to the ceiling, and the point of refusing
# is that a pattern past it no longer answers a question about a narrower one.
def show(n)
  re = Regexp.new("(a)" * n)
  m = re.match("a" * n)
  puts "#{n}: size=#{m.size} last=#{m[n].inspect}"
rescue RegexpError => e
  puts "#{n}: #{e.message[0, 40]}"
end

show(1)
show(15)
show(30)
show(31)
show(32)
show(40)
show(200)

# a literal past the ceiling is refused when the program is compiled, not when
# it runs, so there is nothing here to show for it; see `make re-lit-test`.

# the ceiling counts capture groups, not groups: (?:...) is free
big = "(?:a)" * 100 + "(a)"
m = Regexp.new(big).match("a" * 101)
p [m.size, m[1]]

# and a named group counts the same way
named = (1..31).map { |i| "(?<g#{i}>a)" }.join
p Regexp.new(named).match("a" * 31).size
begin
  Regexp.new((1..32).map { |i| "(?<h#{i}>a)" }.join)
  puts "accepted"
rescue RegexpError => e
  puts e.message[0, 40]
end
