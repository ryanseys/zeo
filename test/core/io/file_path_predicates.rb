p File.join("a/", "b")
p File.join("a", "", "b")
p File.join("a", "/b")
p File.join("", "a")
p File.join("a//", "b")
p File.join("a//", "//b")
p File.join(["a", "b"], "c")
p File.join("a", ["b", ["c"]])
p File.basename("/a/b/")
p File.basename("/")
p File.basename("a/b//")
p File.basename("///")
p File.basename("")
p File.basename("b")
p File.basename("/a/b/c.rb", ".rb")
p File.basename("/a/b/c.rb", ".*")
p File.basename("c.tar.gz", ".*")
p File.basename("c.rb", ".py")
File.write("/tmp/sp_file_zero_empty.txt", "")
p File.size("/tmp/sp_file_zero_empty.txt")
p File.zero?("/tmp/sp_file_zero_empty.txt")
p File.empty?("/tmp/sp_file_zero_empty.txt")
File.delete("/tmp/sp_file_zero_empty.txt")
p File.zero?("/tmp")
p File.zero?("/dev/null")
p File.zero?("/nonexistent_zzz")
r = (File.exists?("/tmp") rescue $!.class); p r
r2 = (Dir.exists?("/tmp") rescue $!.class); p r2
p File::ALT_SEPARATOR
p File::ALT_SEPARATOR.nil?
__END__
"a/b"
"a/b"
"a/b"
"/a"
"a//b"
"a//b"
"a/b/c"
"a/b/c"
"b"
"/"
"b"
"/"
""
"b"
"c"
"c"
"c.tar"
"c.rb"
0
true
true
false
true
false
NoMethodError
NoMethodError
nil
true
