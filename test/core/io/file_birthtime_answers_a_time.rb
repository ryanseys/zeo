# On a file just written, where the platform records one.
require "tmpdir"
ZTMP = Dir.mktmpdir

path = File.join(ZTMP, "probe_issue_2985.txt")
File.write(path, "hello")
t = File.birthtime(path)
puts t.class
puts t.is_a?(Time)
File.delete(path)
__END__
Time
true
