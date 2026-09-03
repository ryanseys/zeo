p File.absolute_path?("/a/b")
p File.absolute_path?("a/b")
p File.absolute_path?("./x")
__END__
true
false
false
