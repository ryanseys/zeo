File.write("/tmp/sp_f2", "hello world")
p File.ftype("/tmp/sp_f2")
Dir.mkdir("/tmp/sp_f2dir") unless Dir.exist?("/tmp/sp_f2dir")
p File.ftype("/tmp/sp_f2dir")
p((File.ftype("/nonex") rescue $!.class))
p File.writable?("/tmp/sp_f2")
p File.executable?("/tmp/sp_f2")
p File.size?("/tmp/sp_f2")
p File.size?("/nonex")
File.write("/tmp/sp_f0", "")
p File.size?("/tmp/sp_f0")
p File.pipe?("/tmp/sp_f2")
p File.identical?("/tmp/sp_f2", "/tmp/sp_f2")
p File.identical?("/tmp/sp_f2", "/tmp/sp_f2dir")
p File.atime("/tmp/sp_f2").class
p File.ctime("/tmp/sp_f2").class
p File.realpath("/tmp/../tmp/sp_f2") == File.join(File.realpath("/tmp"), "sp_f2")
p File.read("/tmp/sp_f2", 5)
p File.read("/tmp/sp_f2", 500)
p File.chmod(0644, "/tmp/sp_f2")
p File.truncate("/tmp/sp_f2", 5)
p File.read("/tmp/sp_f2")
File.write("/tmp/sp_f2", "hello")
File.write("/tmp/sp_f2", "XY", 1)
p File.read("/tmp/sp_f2")
p File.write("/tmp/sp_f2", "AB", mode: "a")
p File.read("/tmp/sp_f2")
File.foreach("/tmp/sp_f2") { |line| p line }
p File.split("/a/b/c.rb")
p File.absolute_path("c.rb", "/a/b")
p File.path("/a/b")
p File.fnmatch("*.rb", "c.rb")
p File.fnmatch("a*", "bc")
p File.fnmatch?("*.rb", "x.rb")
f = File.new("/tmp/sp_f2")
p f.read
f.close
f2 = File.open("/tmp/sp_f2")
p f2.size
p f2.mtime.class
p f2.chmod(0644)
f2.close
st = File.stat("/tmp/sp_f2")
p st.size
p st.mtime.class
p001 = "/tmp/sp_bug_open_intmode"
File.open(p001, File::WRONLY | File::CREAT | File::TRUNC) { |f| f.write("x") }
p File.read(p001)
File.delete(p001)
r001 = (begin; File.open("/tmp/sp_modekw", mode: "w") { |f| f.write("x") }; File.read("/tmp/sp_modekw"); rescue => e001; e001.class; end)
p r001
File.delete("/tmp/sp_modekw") if File.exist?("/tmp/sp_modekw")
p FileTest.exist?("/tmp/sp_f2")
p FileTest.file?("/tmp/sp_f2")
p FileTest.directory?("/tmp/sp_f2dir")
p File.dirname("/a/b/c", 2)
p File.dirname("a/b/c/d", 3)
p File.dirname("/a", 5)
File.delete("/tmp/sp_f2", "/tmp/sp_f0")
Dir.rmdir("/tmp/sp_f2dir")
puts "done"
