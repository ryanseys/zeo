# Originally, only a literal-on-literal `+` compiled at all. This
# proves the generalization: arithmetic on locals typed `Int` by the
# forward local-type tracker (`analyze::locals`).

x = 5
y = 3
puts x + y
puts x - y
puts x * y
puts x / y
puts x % y
__END__
8
2
15
1
2
