S = Struct.new(:a, :b, :c)
s = S.new(1, 2, 3)

# each_pair yields the [name, value] PAIR, so a one-parameter block sees it
# whole; it was yielding two values, and the 1-param form got only the name
s.each_pair { |pair| p pair }
s.each_pair { |k, v| p [k, v] }

# blockless each_pair answers an Enumerator over those pairs, not LocalJumpError
r = (s.each_pair.to_a rescue $!.class); p r
p s.each_pair.class

# values_at pads a Range that runs past the last member, as Array#values_at does
T = Struct.new(:a, :b)
p T.new(1, 2).values_at(0..5)
p T.new(1, 2).values_at(0, 1)
p T.new(1, 2).values_at(1)
p [1, 2].values_at(0..5)

# an anonymous Struct has no name
p Struct.new(:z).name
p S.name

# a keyword_init Struct takes keywords and nothing else
K = Struct.new(:a, :b, keyword_init: true)
r = (K.new(1, 2) rescue $!.class); p r
p K.new(a: 1, b: 2).a
p K.keyword_init?

# a plain Struct still takes positionals
p S.new(1, 2, 3).to_a
__END__
[:a, 1]
[:b, 2]
[:c, 3]
[:a, 1]
[:b, 2]
[:c, 3]
[[:a, 1], [:b, 2], [:c, 3]]
Enumerator
[1, 2, nil, nil, nil, nil]
[1, 2]
[2]
[1, 2, nil, nil, nil, nil]
nil
"S"
ArgumentError
1
true
[1, 2, 3]
