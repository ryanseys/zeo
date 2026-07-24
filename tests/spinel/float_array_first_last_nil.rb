# FloatArray#first/#last on an empty array return nil (float? nullable),
# not 0.0. A non-empty array yields the value as a plain float, and the
# value flows transparently through arithmetic (the float? rides as a
# double; base_type strips the `?`).
a = [1.5, 2.5]
p a.first
p a.last
p a.first.nil?

b = [3.0, 4.0]
b.pop
b.pop
p b.first
p b.last
p b.first.nil?

c = [10.5, 4.5]
p c.last - c.first        # float? - float? -> float
x = c.last
p x + 1.0                 # float? flowing into bare-float arithmetic
