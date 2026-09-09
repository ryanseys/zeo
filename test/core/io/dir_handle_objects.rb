# Dir.new/Dir.open handles: #read/#each/#each_child/#children/#entries/
# #path/#rewind/#close, the block form, and Errno::ENOENT on a missing path.
require "tmpdir"
ZTMP = Dir.mktmpdir

dir = File.join(ZTMP, "sp_dirh")
Dir.mkdir(dir) unless Dir.exist?(dir)
File.write("#{dir}/x", "")
File.write("#{dir}/y", "")
d = Dir.new(dir)
p d.class
p File.basename(d.path)
e = []
d.each { |n| e << n }
p e.sort
d.rewind
p d.read.class
d.close
p Dir.open(dir).class
r = Dir.open(dir) { |dd| dd.children.sort }
p r
p((Dir.new("/nonexistent_zz") rescue $!.class))
cs = []
d2 = Dir.new(dir)
d2.each_child { |n| cs << n }
p cs.sort
p d2.entries.sort
d2.close
File.delete("#{dir}/x", "#{dir}/y")
Dir.rmdir(dir)
puts "done"
__END__
Dir
"sp_dirh"
[".", "..", "x", "y"]
String
Dir
["x", "y"]
Errno::ENOENT
["x", "y"]
[".", "..", "x", "y"]
done
