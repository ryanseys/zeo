# `p` of a bignum-producing expression must print the integer, just as
# `puts`/`print` already do (Integer#inspect == #to_s). Previously the
# `p` argument emitter had no TY_BIGINT case and rejected these.
p(10**30)
p(2**100)
p(10 ** 20 + 1)
p(2**64)
p(3**50)
__END__
1000000000000000000000000000000
1267650600228229401496703205376
100000000000000000001
18446744073709551616
717897987691852588770249
