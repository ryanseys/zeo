# rotate! advances the array's own start offset and copies only the head to the
# tail, instead of buffering the head, sliding the rest down and copying back.
# Everything that reads the array has to keep working across that bump: the
# window is no longer at offset zero.
a = [1, 2, 3, 4, 5, 6, 7, 8]
a.rotate!(3)
p a
a.rotate!(3)
p a
a.rotate!(-2)
p a
p a.length
p a[0]
p a[-1]
p a[2, 3]
p a.first
p a.last

# repeated rotations, which is what exhausts the headroom and forces the slide
b = (0...16).to_a
12.times { b.rotate!(8) }
p b
p b.length

# the mutators, after the window has moved
c = [1, 2, 3, 4]
c.rotate!(1)
c.push(99)
p c
p c.pop
c.unshift(0)
p c
p c.shift
p c

d = [10, 20, 30, 40, 50, 60]
d.rotate!(2)
d[1, 2] = [77, 88]
p d
d[0, 4] = [1]
p d

# reads that walk the whole window
e = [5, 1, 4, 2, 3]
e.rotate!(3)
p e.sort
p e.include?(4)
p e.index(2)
p e.sum
p e.min
p e.max
p e.reverse
p e.map { |x| x * 2 }
p e.each_slice(2).to_a
p e.dup
p e.to_a
p e.inspect

# degenerate rotations
f = [1, 2, 3]
f.rotate!(0)
p f
f.rotate!(3)
p f
f.rotate!(300)
p f
g = []
g.rotate!(5)
p g
h = [7]
h.rotate!(1)
p h

# a large one keeps the in-place path (no headroom growth)
big = (0...600).to_a
big.rotate!(1)
p big.first(3)
p big.last(3)
p big.length
