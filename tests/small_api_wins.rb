require "set"
m = "2026-06".match(/(\d+)-(\d+)/)
p m.size
p m.length
p Set[1, 2, 3].join("-")
p Complex::I
p(Complex::I * Complex::I)
p 5.method(:+).source_location
p 5.method(:+).super_method
