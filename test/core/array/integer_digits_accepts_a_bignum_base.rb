# A bignum base is valid (the value simply fits in one digit when it is
# smaller than the base); only radix < 2 is an ArgumentError.

b = 2 ** 70
p 255.digits(b)
p 255.digits(b).class
__END__
[255]
Array
