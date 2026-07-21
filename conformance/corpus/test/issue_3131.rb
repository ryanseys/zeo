p001 = "sp_io_3131.tmp"
File.write(p001, "hi")
File.open(p001) { |f| f.binmode; p f.binmode? }
File.open(p001) { |f| f.autoclose = false; p f.autoclose? }
File.open(p001) { |f| buf = +""; f.pread(2, 0, buf); p buf }
File.write("sp_io_3131b.tmp", "other")
File.open(p001) { |f| File.open("sp_io_3131b.tmp") { |g| f.reopen(g); p f.read } }
File.delete(p001); File.delete("sp_io_3131b.tmp")
