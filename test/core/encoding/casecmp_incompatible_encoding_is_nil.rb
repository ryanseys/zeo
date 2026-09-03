# `String#casecmp` and `#casecmp?` against an incompatibly-encoded operand
# answer nil; zeo compares the bytes anyway and answers an ordering.
# (Found by the 2026-08-24 probe sweep.)
p "abc".casecmp("ABC".encode("UTF-16LE"))
p "abc".casecmp?("ABC".encode("UTF-16LE"))
__END__
nil
nil
