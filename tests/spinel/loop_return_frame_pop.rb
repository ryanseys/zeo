# A `return` from inside `loop { }` has to pop the handler frame that loop
# opened (it rescues StopIteration through a setjmp). Without the accounting
# the frame stack grew by one per call and, past its 64 slots, the writes ran
# off the end of the array -- the GC then walked a corrupted root stack and
# the program segfaulted.

def first_at_least(n)
  i = 0
  loop do
    i += 1
    loop do
      return i if i >= n
      break
    end
  end
end

200.times { first_at_least(3) }
GC.start
p first_at_least(3)
p first_at_least(7)

# the same shape with the loop's value used (the value-position emitter)
def pick(n)
  i = 0
  v = loop do
    i += 1
    return -1 if i > n
    break i if i == n
  end
  v
end

200.times { pick(4) }
GC.start
live = (1..50).map { |i| [i.to_s, i] }
GC.start
p live.length
p pick(4)
p pick(0)
