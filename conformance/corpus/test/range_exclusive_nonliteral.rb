# Exclusive ranges (`...`) held in a variable carry exclude_end at
# runtime via the sp_Range struct, so max / include? / to_a / each /
# map / size / step all stop one short of the end — matching CRuby
# and the literal-range forms.
r = 1...5
p r.max
p r.min
p r.first
p r.last
p r.to_a
p r.include?(5)
p r.include?(4)
p r.cover?(5)
p r.size
p r.count

sum = 0
r.each { |i| sum += i }
p sum
p r.map { |i| i * 2 }

# Literal exclusive forms were also affected by the shared max /
# include? codegen path.
p((1...5).max)
p((1...5).include?(5))
p((1...5).to_a)

# Inclusive ranges are unchanged.
ri = 1..5
p ri.max
p ri.last
p ri.to_a
p ri.include?(5)
p ri.size
