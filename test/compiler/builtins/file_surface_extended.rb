require "tmpdir"
ZTMP = Dir.mktmpdir

File.write(File.join(ZTMP, "sp_f2"), "hello world")
p File.ftype(File.join(ZTMP, "sp_f2"))
Dir.mkdir(File.join(ZTMP, "sp_f2dir")) unless Dir.exist?(File.join(ZTMP, "sp_f2dir"))
p File.ftype(File.join(ZTMP, "sp_f2dir"))
p((File.ftype("/nonex") rescue $!.class))
p File.writable?(File.join(ZTMP, "sp_f2"))
p File.executable?(File.join(ZTMP, "sp_f2"))
p File.size?(File.join(ZTMP, "sp_f2"))
p File.size?("/nonex")
File.write(File.join(ZTMP, "sp_f0"), "")
p File.size?(File.join(ZTMP, "sp_f0"))
p File.pipe?(File.join(ZTMP, "sp_f2"))
p File.identical?(File.join(ZTMP, "sp_f2"), File.join(ZTMP, "sp_f2"))
p File.identical?(File.join(ZTMP, "sp_f2"), File.join(ZTMP, "sp_f2dir"))
p File.atime(File.join(ZTMP, "sp_f2")).class
p File.ctime(File.join(ZTMP, "sp_f2")).class
# realpath resolves `..`, so a path that walks out of the scratch directory
# and back in answers the same as the direct one.
p File.realpath(File.join(ZTMP, "..", File.basename(ZTMP), "sp_f2")) ==
  File.realpath(File.join(ZTMP, "sp_f2"))
p File.read(File.join(ZTMP, "sp_f2"), 5)
p File.read(File.join(ZTMP, "sp_f2"), 500)
p File.chmod(0644, File.join(ZTMP, "sp_f2"))
p File.truncate(File.join(ZTMP, "sp_f2"), 5)
p File.read(File.join(ZTMP, "sp_f2"))
File.write(File.join(ZTMP, "sp_f2"), "hello")
File.write(File.join(ZTMP, "sp_f2"), "XY", 1)
p File.read(File.join(ZTMP, "sp_f2"))
p File.write(File.join(ZTMP, "sp_f2"), "AB", mode: "a")
p File.read(File.join(ZTMP, "sp_f2"))
File.foreach(File.join(ZTMP, "sp_f2")) { |line| p line }
p File.split("/a/b/c.rb")
p File.absolute_path("c.rb", "/a/b")
p File.path("/a/b")
p File.fnmatch("*.rb", "c.rb")
p File.fnmatch("a*", "bc")
p File.fnmatch?("*.rb", "x.rb")
f = File.new(File.join(ZTMP, "sp_f2"))
p f.read
f.close
f2 = File.open(File.join(ZTMP, "sp_f2"))
p f2.size
p f2.mtime.class
p f2.chmod(0644)
f2.close
st = File.stat(File.join(ZTMP, "sp_f2"))
p st.size
p st.mtime.class
p001 = File.join(ZTMP, "sp_bug_open_intmode")
File.open(p001, File::WRONLY | File::CREAT | File::TRUNC) { |f| f.write("x") }
p File.read(p001)
File.delete(p001)
r001 = (begin; File.open(File.join(ZTMP, "sp_modekw"), mode: "w") { |f| f.write("x") }; File.read(File.join(ZTMP, "sp_modekw")); rescue => e001; e001.class; end)
p r001
File.delete(File.join(ZTMP, "sp_modekw")) if File.exist?(File.join(ZTMP, "sp_modekw"))
p FileTest.exist?(File.join(ZTMP, "sp_f2"))
p FileTest.file?(File.join(ZTMP, "sp_f2"))
p FileTest.directory?(File.join(ZTMP, "sp_f2dir"))
p File.dirname("/a/b/c", 2)
p File.dirname("a/b/c/d", 3)
p File.dirname("/a", 5)
File.delete(File.join(ZTMP, "sp_f2"), File.join(ZTMP, "sp_f0"))
Dir.rmdir(File.join(ZTMP, "sp_f2dir"))
puts "done"
__END__
"file"
"directory"
Errno::ENOENT
true
false
11
nil
nil
false
true
false
Time
Time
true
"hello"
"hello world"
1
0
"hello"
"hXYlo"
2
"hXYloAB"
"hXYloAB"
["/a/b", "c.rb"]
"/a/b/c.rb"
"/a/b"
true
false
true
"hXYloAB"
7
Time
0
7
Time
"x"
"x"
true
true
true
"/a"
"a"
"/"
done
