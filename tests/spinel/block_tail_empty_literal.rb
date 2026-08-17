[].then { [] }.tap { p 0 }

p([].then { [] })
p([1].then { [] })
p(1.then { [] })
p("s".then { [] })
p([].then { {} })
p([].then { [] }.size)
p([].then { [] } + [1])
p([].yield_self { [] })

x = [].then { [] }
p x
p x.class

p([].then { [1] })
p([].tap { [] })
p([1].map { [] })
p([1, 2].each_with_object([]) { |e, m| m << e })

a = [1, 2].map { [] }
a[0] << 1
p a

pr = proc { [] }
p pr.call
l = -> { [] }
p l.call
