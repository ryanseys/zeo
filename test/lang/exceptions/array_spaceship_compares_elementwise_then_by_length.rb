# Array#<=> compares element by element and falls back to length, and answers 0 for two empties.
r = ([] <=> [] rescue $!.class); p r
p([1, 2] <=> [1, 2])
p([1] <=> [1, 2])
p([1, 2, 3] <=> [])
__END__
0
0
-1
1
