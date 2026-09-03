p "abcd".unpack("x2a*")
p "abc".unpack("@1a*")
p "abc".unpack("a1@0a1")
__END__
["cd"]
["bc"]
["a", "a"]
