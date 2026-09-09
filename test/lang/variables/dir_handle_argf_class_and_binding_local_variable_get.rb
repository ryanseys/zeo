# Dir.new/Dir.open handles (#path/#read/#each/#children/#entries/
# #rewind/#close, the block form, ENOENT on a missing path), ARGF's
# literally-named class and default "-" filename with no file args, and
# binding.local_variable_get(:name) reading an in-scope local (including a
# reserved-word parameter).
require "tmpdir"
ZTMP = Dir.mktmpdir


dir = File.join(ZTMP, "sp_e2e_dirh_#{Process.pid}")
Dir.mkdir(dir) unless Dir.exist?(dir)
File.write("#{dir}/x", "")
File.write("#{dir}/y", "")
d = Dir.new(dir)
p d.class
p d.path == dir
names = []
d.each { |n| names << n }
p names.sort
d.rewind
p d.read.class
d.close
r = Dir.open(dir) { |dd| dd.children.sort }
p r
p((Dir.new("/nonexistent_zz_e2e") rescue $!.class))
File.delete("#{dir}/x", "#{dir}/y")
Dir.rmdir(dir)

p ARGF.class
p ARGF.filename

def read_reserved(then:)
  binding.local_variable_get(:then)
end
p read_reserved(then: :tick)
x = [1, 2, 3]
p binding.local_variable_get(:x)
__END__
Dir
true
[".", "..", "x", "y"]
String
["x", "y"]
Errno::ENOENT
ARGF.class
"-"
:tick
[1, 2, 3]
