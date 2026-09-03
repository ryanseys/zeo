# A trailing comma (`|a, |`) means "this block takes more than one param",
# which turns auto-splat on and then discards everything past the named
# ones -- exactly an anonymous rest, which is how it lowers.

def one(x) = yield x
one([1, 2]) { |a,| p a }
one([1, 2, 3]) { |a, b,| p [a, b] }
__END__
1
[1, 2]
