b = true
p(TrueClass === b)
p(case b when TrueClass then :yes else :no end)
f = false
p(FalseClass === f)
p(TrueClass === f)
p(Integer === 5)
p(Integer === b)
