# `String#casecmp` and `#casecmp?` against an incompatibly-encoded operand
# answer nil; zeo compares the bytes anyway and answers an ordering.
p "abc".casecmp("ABC".encode("UTF-16LE"))
p "abc".casecmp?("ABC".encode("UTF-16LE"))
__END__
nil
nil
