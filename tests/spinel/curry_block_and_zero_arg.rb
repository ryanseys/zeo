add001 = ->(a, b) { a + b }.curry[10]
p [1, 2, 3].map(&add001)
p(->() { 5 }.curry.call)
c002 = ->() { 5 }.curry
p c002.call
p c002[]
mul = ->(a, b) { a * b }.curry
p [1, 2, 3].select(&mul[1]).class
p [4, 5].map(&->(a, b) { a * b }.curry[3])
