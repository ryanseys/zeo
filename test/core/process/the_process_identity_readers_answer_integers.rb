# uid, gid, euid, egid, getpgrp and getsid.
# (spinel issue #3043)
puts Process.uid.class
puts Process.gid.class
puts Process.euid.class
puts Process.egid.class
puts Process.getpgrp.class
puts Process.getsid.class
puts Process.getsid(0).class
puts(Process.uid == Process.euid)
__END__
Integer
Integer
Integer
Integer
Integer
Integer
Integer
true
