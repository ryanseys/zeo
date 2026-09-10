# True for a rooted path, false for a relative one and for a dot-relative one.
p File.absolute_path?("/a/b")
p File.absolute_path?("a/b")
p File.absolute_path?("./x")
__END__
true
false
false
