# `param = "x" if assign` leaves param defined and nil when the guard is false.
# (spinel issue #3295)
def value(assign)
  param = "x" if assign
  param
end

raise "FAIL" unless value(false).nil?
p value(false)
p value(true)
x = "set" if false
p x
p x.nil?
__END__
nil
"x"
nil
true
