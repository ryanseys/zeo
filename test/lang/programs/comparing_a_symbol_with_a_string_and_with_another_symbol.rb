# `<=>` answers nil against a String and an Integer, and orders two Symbols.
p(:abc <=> "abc")
p(:abc <=> "abd")
a = :abd; b = "abc"; p(a <=> b)
p(:abc <=> :abd)
p(:abc <=> 1)
p([:b, :a].sort)
__END__
nil
nil
nil
-1
nil
[:a, :b]
