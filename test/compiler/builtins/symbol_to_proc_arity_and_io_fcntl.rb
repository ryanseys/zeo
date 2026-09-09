# :name.to_proc reports arity -2 (receiver + optional args); IO#fcntl runs
# the raw syscall (F_GETFD reads the close-on-exec flag on the open file).
require "tmpdir"
ZTMP = Dir.mktmpdir


p :upcase.to_proc.arity
p ["x", "y"].map(&:upcase)
pth = File.join(ZTMP, "sp_e2e_fcntl_#{Process.pid}.tmp")
File.write(pth, "hi")
File.open(pth) { |f| p(f.fcntl(1, 0).class) }
File.delete(pth)
__END__
-2
["X", "Y"]
Integer
