add001 = ->(a, b) { a + b }.curry
c001 = add001[1] >> ->(x001) { x001 * 10 }
p c001.call(4)

add002 = ->(a, b) { a + b }.curry; p((add002[1] >> ->(x002) { x002 * 10 }).call(4))  # Ruby: 50
c003 = ->(a) { a + 1 }.curry; p((c003 >> ->(x003) { x003 * 10 }).call(4))            # Ruby: 50
c004 = ->(a) { a + 1 }.curry; p((c004 << ->(x004) { x004 * 10 }).call(4))            # Ruby: 41

add005 = ->(a, b) { a + b }.curry
p add005[1][2]                                                   # => 3
p add005[1].call(2)                                              # => 3
c006 = ->(x006) { x006 + 1 } >> ->(x007) { x007 * 10 }
p c006.call(4)                                                   # => 50
p((->(x008) { x008 + 1 } >> ->(x009) { x009 * 10 }).call(4))     # => 50

mul = ->(a, b) { a * b }.curry
inc = ->(x) { x + 1 }
p((inc >> mul[3]).call(4))
p((mul[2] << inc).call(4))
p(mul[2][5])
