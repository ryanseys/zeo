fs001 = [->(x) { x + 1 }]
id001 = ->(x) { x }
c001 = id001 >> fs001[0]
p c001.call(4)
h = { f: ->(x) { x * 2 } }
c003 = id001 >> h[:f]
p c003.call(3)
p (id001 >> fs001[0]).call(1)
