# `[1, 2][1..2**70]` and `"ab"[1..2**70]` answer `nil` where ruby raises
# `RangeError: bignum too big to convert into 'long'`.
#
# The index conversion narrows a `BigInt` to a machine integer and treats
# the overflow as "out of bounds", so the call falls into the ordinary
# nil-for-a-missing-slice answer instead of reporting that the index itself
# is unrepresentable. Ruby draws the line the other way: an index that
# cannot be a `long` is an ERROR, not a miss, because the two mean different
# things to a caller -- `nil` says "no such element", `RangeError` says "that
# is not an index".
#
# It is narrow, and the shape of what still agrees says how narrow: a bare
# bignum index (`[1,2][2**70]`), a bignum LENGTH (`slice(0, 2**70)`), and
# `first`/`take`/`drop`/`fill`/`rotate`/`values_at`/`dig` with one all match
# ruby already. Only the RANGE form of the index is wrong, on `Array` and on
# `String`.
#
# Its sibling is `a_bignum_range_endpoint_is_not_an_integer_range.rb`: this
# file is about a range zeo declines to convert, that one about a range zeo
# declines to walk.
#
# Oracle: a bignum index raises.
B = 2**70
begin
  p [1, 2][1..B]
rescue RangeError => e
  p [:ary, e.class, e.message]
end
begin
  p "ab"[1..B]
rescue RangeError => e
  p [:str, e.class, e.message]
end
p [1, 2][B].inspect
p [1, 2].slice(0, B)
p [1, 2].first(B)
p [1, 2].values_at(B)
