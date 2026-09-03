# Arbitrary-precision integers: the bignum library is linked and the results
# are exact.
puts (2**200).to_s
puts (3**64) / 7
puts 100.downto(1).reduce(:*).to_s.length
puts (10**30 + 1) % 97
__END__
1606938044258990275541962092341162602522202993782792835301376
490526260041787497808264155611
158
86
