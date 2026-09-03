# `(a; b)` introduces NO scope: `a` below is still readable afterwards,
# which is why this lowers to a plain `Seq` rather than anything that
# pushes a scope.

x = (1; 2; 3)
p x
y = (a = 5; a * 2)
p y
p a
p((puts "side"; 42))
p [(1; 2), 3]
z = (
  q = 7
  q + 1
)
p z
p((1; (2; 3)))
w = (if true then "yes" else "no" end; "after")
p w
p (5)
__END__
3
10
5
side
42
[2, 3]
8
3
"after"
5
