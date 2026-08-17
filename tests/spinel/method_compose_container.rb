fs001 = [->(x001) { x001 + 1 }]
m001 = 2.method(:*)
c001 = m001 >> fs001[0]
p c001.call(4)

fs002 = [->(x002) { x002 + 1 }]; m002 = 2.method(:*); p((m002 >> fs002[0]).call(4))  # Ruby: 9
fs003 = [->(x003) { x003 + 1 }]; m003 = 2.method(:*); p((m003 << fs003[0]).call(4))  # Ruby: 6

fs004 = [->(x004) { x004 + 1 }, ->(x004b) { x004b * 3 }]; id004 = ->(x004c) { x004c }
p(fs004.reduce(id004, :>>).call(4))   # Ruby: 15

fs006 = [->(x006) { x006 + 1 }]
id006 = ->(x006b) { x006b }
c006 = id006 >> fs006[0]
p c006.call(4)                                   # => 5
p((id006 << fs006[0]).call(4))                   # => 5
m007 = 2.method(:*)
p((m007 >> ->(x007) { x007 + 1 }).call(4))       # => 9
fs008 = [->(x008) { x008 + 1 }, ->(x008b) { x008b * 3 }]
id008 = ->(x008c) { x008c }
p(fs008.inject(id008) { |a008, b008| a008 >> b008 }.call(4))   # => 15
p(fs008.reduce(:>>).call(4))                                   # => 15

fs = [->(x) { x * 2 }, ->(x) { x + 3 }]
p(fs.reduce(->(x) { x }, :<<).call(2))
m = 3.method(:+)
h = { f: ->(x) { x * 10 } }
p((m >> h[:f]).call(1))
p((m << h[:f]).call(1))
