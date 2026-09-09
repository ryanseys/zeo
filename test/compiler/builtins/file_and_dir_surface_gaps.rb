# Dir.glob with an ABSOLUTE-path pattern + a `*` wildcard (used to return
# [] because the walk read "" instead of "/"); FNM_DOTMATCH yields "." and
# dotfiles but never ".."; File.mkfifo returns 0 and creates a FIFO;
# File#lstat returns a File::Stat; File.exists? was removed in Ruby 3.2.
require "tmpdir"
ZTMP = Dir.mktmpdir


d = File.join(ZTMP, "sp_e2e_fdir_#{Process.pid}")
Dir.mkdir(d) unless Dir.exist?(d)
File.write("#{d}/a1", ""); File.write("#{d}/a2", ""); File.write("#{d}/.hid", "")
p Dir.glob("#{d}/*").map { |x| x.sub("#{d}/", "") }.sort
p Dir.glob("#{d}/*", File::FNM_DOTMATCH).map { |x| x.sub("#{d}/", "") }.sort
fifo = "#{d}/f"
p File.mkfifo(fifo)
p File.stat(fifo).ftype
p File.open("#{d}/a1") { |f| f.lstat.class }
r = (begin; File.exists?("#{d}"); rescue => e; e.class; end); p r
File.delete("#{d}/a1"); File.delete("#{d}/a2"); File.delete("#{d}/.hid"); File.delete(fifo)
Dir.rmdir(d)
__END__
["a1", "a2"]
[".", ".hid", "a1", "a2"]
0
"fifo"
File::Stat
NoMethodError
