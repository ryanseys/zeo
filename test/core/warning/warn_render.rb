warn ["x", "y"]
warn "z"
warn 1, 2
warn []
warn nil
warn :s
warn({ a: 1 })
warn "t\n"
warn ["a", ["b", "c"]]
warn 1.5

__END__
#@ stderr
x
y
z
1
2

s
{a: 1}
t
a
b
c
1.5
