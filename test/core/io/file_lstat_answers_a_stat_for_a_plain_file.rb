# Its class, size and ftype.
require "tmpdir"
ZTMP = Dir.mktmpdir

p001 = File.join(ZTMP, "sp_issue_2986.tmp")
File.write(p001, "xyz")
s001 = File.lstat(p001)
p s001.class
p s001.size
p s001.ftype
File.symlink(p001, File.join(ZTMP, "sp_issue_2986.link")) rescue nil
l = File.lstat(File.join(ZTMP, "sp_issue_2986.link"))
p l.ftype
p File.stat(File.join(ZTMP, "sp_issue_2986.link")).ftype
File.open(p001) { |f| p f.lstat.class }
File.delete(File.join(ZTMP, "sp_issue_2986.link"))
File.delete(p001)
__END__
File::Stat
3
"file"
"link"
"file"
File::Stat
