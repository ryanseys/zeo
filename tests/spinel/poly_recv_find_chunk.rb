# find / detect answer the winning ELEMENT of a receiver only known to be a
# container at run time, and the chunk family answers an Enumerator unless a
# .to_a terminal materializes the runs. Both had no arm for a poly receiver and
# fell through to the last-resort Hash face, which types find as the [k, v]
# pair -- a type the emitter never renders.
rows = [[1, 2, 4, 5], { "a" => 1, "b" => 2 }, "x"]

a = rows[0]
p a.find { |x| x > 2 }
p a.detect { |x| x > 2 }
p a.chunk { |x| x > 2 }.to_a
p a.slice_before { |x| x > 3 }.to_a
p a.slice_after { |x| x > 3 }.to_a

# a Hash answers its [k, v] pairs through the same loop
h = rows[1]
p h.find { |k, n| n > 1 }
p h.detect { |k, n| n > 1 }

def pick(n) = n > 0 ? [1, 2, 4, 5] : nil
[1, 0].each do |k|
  v = pick(k)
  p v&.find { |x| x > 2 }
  p v&.detect { |x| x > 2 }
  r = v&.chunk { |x| x > 2 }
  p(r.nil? ? nil : r.to_a)
end
