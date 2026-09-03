p 5.rect
p 5.polar
p((-5).polar)
p 2.5.polar
p 5.rationalize
p Rational(3, 2).polar
p(nil =~ /x/)
p [1, 2, 3, 4].rfind { |x| x.even? }
p [10, 20, 30].fetch_values(0, 2)
p([10, 20, 30].fetch_values(0, 5) { |i| i * 100 })
p({ a: 1, b: 2 }.to_proc.call(:b))
p({ a: 1, b: 2 }.transform_keys!(&:to_s))
p((1..5).overlap?(5..8))
p((1...5).overlap?(5..8))
p((1..5).overlap?(6..8))
__END__
[5, 0]
[5, 0]
[5, 3.141592653589793]
[2.5, 0]
(5/1)
[(3/2), 0]
nil
4
[10, 30]
[10, 500]
2
{"a" => 1, "b" => 2}
true
false
false
