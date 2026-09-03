# `File`'s pure-path family: string work that never touches the disk. Every
# expectation oracle-read from ruby 4.0.6 -- including the two that read
# like off-by-ones (a TRAILING dot IS an extension, a LEADING one is not).

puts File.basename("/home/user/notes.md")
puts File.basename("/home/user/notes.md", ".md")
puts File.basename("/home/user/notes.md", ".*")
puts File.basename("/a/b/")
puts File.basename("/")
puts File.dirname("/home/user/notes.md")
puts File.dirname("solo")
puts File.dirname("/x")
p File.extname("archive.tar.gz")
p File.extname(".bashrc")
p File.extname("trailing.")
p File.extname("plain")
p File.split("/a/b/c.rb")
__END__
notes.md
notes
notes
b
/
/home/user
.
/
".gz"
""
"."
""
["/a/b", "c.rb"]
