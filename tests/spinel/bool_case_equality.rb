# TrueClass#=== / FalseClass#=== is case equality: true only when the
# argument is a boolean with the same value.
p(true === true)
p(true === false)
p(true === 1)
p(false === false)
p(false === true)
x = true
p(x === true)
p(x === false)
