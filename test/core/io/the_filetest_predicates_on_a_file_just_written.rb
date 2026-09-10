# owned?, exist?, file? and directory?.
require "tmpdir"
ZTMP = Dir.mktmpdir

f = File.join(ZTMP, "sp_ft_2997_#{Process.pid}")
File.write(f, "hi")
p FileTest.owned?(f)
p FileTest.exist?(f)
p FileTest.file?(f)
p FileTest.directory?(f)
p FileTest.symlink?(f)
p FileTest.zero?(f)
p FileTest.setuid?(f)
File.delete(f)
__END__
true
true
true
false
false
false
false
