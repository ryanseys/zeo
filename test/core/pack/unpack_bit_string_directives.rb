p "abcd".unpack("B8")
p "abcd".unpack("b8")
p "abcd".unpack("B*").first.length
__END__
["01100001"]
["10000110"]
32
