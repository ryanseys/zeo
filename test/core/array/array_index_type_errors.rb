# Array#[]= on a frozen array raises TypeError for the bad index before
# FrozenError for the frozen receiver; ruby checks frozenness first.
#
r = ([1, 2].values_at("x") rescue $!.class); p r
s = "x"; r = ([1, 2].values_at(s) rescue $!.class); p r
r = ([1, 2].values_at([0]) rescue $!.class); p r
r = ([1, 2].at("x") rescue $!.class); p r
r = ([1, 2]["x"] rescue $!.class); p r
r = ([1, 2].fetch("x") rescue $!.class); p r
r = ([1, 2].first("x") rescue $!.class); p r
r = ([1, 2].last(:s) rescue $!.class); p r
r = ([1, 2].take(nil) rescue $!.class); p r
r = ([1, 2].drop(true) rescue $!.class); p r
r = ([1, 2].slice("x") rescue $!.class); p r
r = ([1, 2].dig("x") rescue $!.class); p r
r = ([1, 2].rotate(nil) rescue $!.class); p r
r = ([1, 2].at(1..2) rescue $!.class); p r
a = [1, 2]; r = (a.insert("x", 9) rescue $!.class); p r
a = [1, 2]; r = ((a["x"] = 9) rescue $!.class); p r

# the forms that stay legal
p([1, 2].at(1))
p([1, 2][0..1])
p([1, 2].slice(0, 2))
p([1, 2].values_at(0..1))
p([1, 2].first(1.0))
p([1, 2].at(1.0))
p([1, 2].rotate(1))

# a mutator checks frozen before it coerces the index
f = [1, 2, 3].freeze
r = ((f[:foo] = 1) rescue $!.class); p r
r = (f.insert(:x, 9) rescue $!.class); p r
__END__
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
2
[1, 2]
[1, 2]
[1, 2]
[1]
2
[2, 1]
FrozenError
FrozenError
