# Over an array and over a hash, with a hash and an array memo.
# (spinel issue #3253)
[1, 2].each_with_object({}) { |x, acc| p x }
{ a: 1 }.each_with_object({}) { |(k, v), h| p k }
{ a: 1, b: 2 }.each_with_object([]) { |(k, v), acc| p v }
__END__
1
2
:a
1
2
