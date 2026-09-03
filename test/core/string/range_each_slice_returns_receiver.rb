p((1..5).each_cons(2) { |s| s })
p((1..5).each_slice(2) { |s| s })
p([1, 2, 3].each_cons(2) { |s| s })
p([1, 2, 3].each_slice(2) { |s| s })
p({ a: 1, b: 2 }.each_cons(2) { |s| s })
p({ a: 1, b: 2 }.each_slice(2) { |s| s })
p((1..5).to_a.each_slice(2) { |s| s })
__END__
1..5
1..5
[1, 2, 3]
[1, 2, 3]
{a: 1, b: 2}
{a: 1, b: 2}
[1, 2, 3, 4, 5]
