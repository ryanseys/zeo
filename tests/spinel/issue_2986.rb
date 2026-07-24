p001 = "sp_issue_2986.tmp"
File.write(p001, "xyz")
s001 = File.lstat(p001)
p s001.class
p s001.size
p s001.ftype
File.symlink(p001, "sp_issue_2986.link") rescue nil
l = File.lstat("sp_issue_2986.link")
p l.ftype
p File.stat("sp_issue_2986.link").ftype
File.open(p001) { |f| p f.lstat.class }
File.delete("sp_issue_2986.link")
File.delete(p001)
