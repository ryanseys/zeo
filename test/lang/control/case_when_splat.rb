# `when *list` spreads the list the way a call site's splat does: a plain
# value is one candidate and nil is none. With a subject each candidate
# answers `===`; without one, its own truth is the test.
def kind(value, candidates)
  case value
  when *candidates then :hit
  else :miss
  end
end

p kind(1, [1, 2])
p kind(9, [1, 2])
p kind(5, 5)
p kind(5, nil)
p kind(5, 4..6)
p kind("s", [Integer, String])

def any_of(*flags)
  case
  when *flags then :yes
  else :no
  end
end

p any_of(false, nil)
p any_of(false, 1)
p any_of

# The candidates run in order and stop at the first hit.
def loud(tag, value)
  puts "test #{tag}"
  value
end

case 2
when *[loud("first", 1)], *[loud("second", 2)], *[loud("third", 3)] then puts "matched"
end
__END__
:hit
:miss
:hit
:miss
:hit
:hit
:no
:yes
:no
test first
test second
matched
