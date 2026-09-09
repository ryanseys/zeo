# Its class and size, and a stat held in a local answering the same.
# (spinel issue #3041)
require "tmpdir"
ZTMP = Dir.mktmpdir

p001 = File.join(ZTMP, "sp_io_stat_repro.txt")
File.write(p001, "hello world\n")
File.open(p001) { |f| p f.stat.class }
File.open(p001) { |f| p f.stat.size }
File.open(p001) do |f|
  a001 = f.stat
  p a001.class
  p a001.size
end
File.delete(p001)
__END__
File::Stat
12
File::Stat
12
