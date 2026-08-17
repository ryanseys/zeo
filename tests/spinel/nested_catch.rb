# A catch whose body's value is another catch: the inner one's setup was
# appended in the middle of the outer's assignment, so the C did not compile.
p(catch(:outer) { catch(:inner) { 1 } })
p(catch(:a) { catch(:b) { throw :b, 5 } })
p(catch(:a) { catch(:b) { throw :a, 9 } })
p(catch(:x) { 3 })
