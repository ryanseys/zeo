a, b = 1, 2
puts a
puts b

a, *b, c = [1, 2, 3, 4, 5]
puts a
puts b
puts c
__END__
1
2
1
2
3
4
5
