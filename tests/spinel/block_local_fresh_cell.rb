# A block-local captured by a nested proc gets a FRESH cell per block
# invocation (#3230): each iteration's closure keeps its own binding.
# Within one iteration the closure and the block body still share the
# cell, and method-level captures stay shared across calls.
p((1..3).map { |b| n = b; -> { n } }.map(&:call))
p((1..3).map { |b| s = "v#{b}"; -> { s } }.map(&:call))
p((1..3).map { |b| f = b * 1.5; -> { f } }.map(&:call))
p((1..3).map { |b| a = [b]; -> { a } }.map(&:call))
acc = []
(1..2).each { |i| x = i; g = -> { x += 10 }; g.call; acc << x }
p acc
y = 0
inc = -> { y += 1 }
inc.call; inc.call
p y
