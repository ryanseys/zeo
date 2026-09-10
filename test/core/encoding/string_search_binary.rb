# String#include?, #index and #rindex are byte-oriented and NUL-transparent.
# A string can carry embedded NULs -- from pack, or a socket read -- so a
# search that stopped at the first NUL would miss what follows it.
z = [0].pack("C*")
s = "user" + z + "app" + z + "tail"
puts s.include?("app" + z)   # needle contains a NUL
puts s.include?("tail")      # plain needle after the haystack's first NUL
puts s.index("tail")
puts s.index("app")
puts s.rindex("app")
# ordinary (NUL-free) searches are unchanged
puts "hello world".include?("lo wo")
puts "hello world hello".index("hello", 3)
puts "hello world hello".rindex("hello")
puts "café テスト".index("テスト")
__END__
true
true
9
5
5
true
12
12
5
