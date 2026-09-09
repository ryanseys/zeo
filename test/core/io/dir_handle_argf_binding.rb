# Dir.new/Dir.open handles, ARGF's literally-named class and default "-"
# filename (run with no file arguments), and binding.local_variable_get reading
# an in-scope local -- including a reserved-word parameter.
require "tmpdir"
ZTMP = Dir.mktmpdir

dir = File.join(ZTMP, "sp_ex_dirh_#{Process.pid}")
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

p Dir.open(dir) { |dd| dd.children.sort }
p Dir.open(dir) { |dd| dd.entries.sort }
p((Dir.new("/nonexistent_zz_example") rescue $!.class))
File.delete("#{dir}/x", "#{dir}/y")
Dir.rmdir(dir)

p ARGF.class
p ARGF.filename

def read_reserved(then:)
  binding.local_variable_get(:then)
end
p read_reserved(then: :tick)

def read_named(name:)
  binding.local_variable_get(:name)
end
p read_named(name: "hello")

x = [1, 2, 3]
p binding.local_variable_get(:x)
__END__
Dir
true
[".", "..", "x", "y"]
String
["x", "y"]
[".", "..", "x", "y"]
Errno::ENOENT
ARGF.class
"-"
:tick
"hello"
[1, 2, 3]
