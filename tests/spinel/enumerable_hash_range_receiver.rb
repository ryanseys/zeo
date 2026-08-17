# Enumerable methods whose only arm is the Array one were refused outright on a
# Hash or a Range receiver. Every one of them answers what the same call on
# `receiver.to_a` answers -- Enumerable over a Hash walks its pairs, over a
# Range its elements -- so that is where they go.
h = { a: 1, b: 2, c: 3 }
r = (1..5)

out = []
h.each_slice(2) { |s| out << s }
p out
p h.each_cons(2).to_a
p h.take_while { |k, v| v < 3 }
p h.drop_while { |k, v| v < 3 }
p h.grep(Array)
p h.grep_v(Array)
p h.sort
p h.minmax_by { |k, v| v }
p h.each_entry { |e| e }

p r.sort
p r.sort { |a, b| b <=> a }
p r.grep(2..4)
p r.grep_v(2..4)
p r.each_slice(2).to_a
p r.each_cons(2).to_a
p r.minmax_by { |x| -x }
p r.reverse_each.to_a
p r.uniq
p r.find_index(3)
p r.zip([9, 8, 7, 6, 5])
p r.flat_map { |x| [x, x] }

# the Hash-returning members keep their own answers
p h.select { |k, v| v > 1 }
p h.reject { |k, v| v > 1 }
p h.map { |k, v| v }
p h.to_a
p r.to_a
