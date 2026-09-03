h = {}
h[:a] = 1
h[:b] = 2
h[:c] = 3
h[:a] = 99
puts h
__END__
{a: 99, b: 2, c: 3}
