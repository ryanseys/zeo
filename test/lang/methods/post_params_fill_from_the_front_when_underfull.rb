# Post params fill left-to-right from whatever the lead/optional/rest slots
# left behind, nil-padding the tail -- they are anchored to the END of the
# argument list only when there are enough values to reach them.

def one(x) = yield x
one([1, 2]) { |a, *b, c, d| p [a, b, c, d] }
one([1, 2, 3, 4, 5]) { |a, *b, c| p [a, b, c] }
one([1, 2]) { |a, b = 5, c = 6, d, e| p [a, b, c, d, e] }
one([1, 2, 3, 4, 5]) { |a, b = 5, c = 6, d, e| p [a, b, c, d, e] }
__END__
[1, [], 2, nil]
[1, [2, 3, 4], 5]
[1, 5, 6, 2, nil]
[1, 2, 3, 4, 5]
