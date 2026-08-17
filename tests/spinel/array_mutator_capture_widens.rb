# Capturing a self-returning Array mutator that widens the element type
# (#3523): the receiver widened to a poly array and the capture kept the
# pre-widening type, so it read the same object at the wrong layout.
a001 = [1, 2]
c001 = a001.push(:x)
p c001
p a001

a002 = [1, 2]
c002 = a002.unshift(:x)
p c002

a003 = [1, 2]
c003 = (a003 << :x)
p c003

a004 = [1, 2]
c004 = a004.concat([:x])
p c004

a005 = [1, 2]
c005 = a005.insert(1, :x)
p c005

a006 = [1, 2]
c006 = a006.push("x")
p c006

# no widening: the capture keeps the narrow type
a007 = [1, 2]
c007 = a007.push(3)
p c007
