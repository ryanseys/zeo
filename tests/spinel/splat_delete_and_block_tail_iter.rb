# Hash#delete with a splatted argument list (#3521): delete was missing from
# the fixed-arity list, so the splat array reached the slot expecting one key.
k001 = [:a]
h001 = { a: 1 }
h001.delete(*k001)
p h001

k002 = [:a]
p({ a: 1 }.delete(*k002))

h003 = { a: 1 }
v003 = h003.delete(*k002)
p v003

# An iterator in the tail position of a BLOCK (#3522): the splice is read as
# the value of a statement expression, and a receiver-returning iterator emits
# as a loop with no value, so the slot got void (or the loop counter).
def y001
  yield({ "a" => 1 })
end
p(y001 { |h| h.each { |k, v| nil } })

def y002
  yield([1, 2])
end
p(y002 { |a| a.each { |x| nil } })

def y003
  yield(1..2)
end
p(y003 { |r| r.each { |x| nil } })

def y004
  yield(2)
end
p(y004 { |n| n.times { |i| nil } })

# consuming it explicitly inside the block kept working
def y005
  yield({ "a" => 1 })
end
p(y005 { |h| r = h.each { |k, v| nil }; r })
