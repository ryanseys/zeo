require "etc"
pw = Etc.getpwuid            # current effective uid
puts pw.class
puts pw.is_a?(Etc::Passwd)
puts pw.name.is_a?(String)
puts pw.uid.is_a?(Integer)
puts pw.gid.is_a?(Integer)
puts pw.dir.is_a?(String)
puts pw.shell.is_a?(String)
# getpwnam(name) round-trips back to the same uid.
puts(Etc.getpwnam(pw.name).uid == pw.uid)
# getpwuid(uid) round-trips back to the same name.
puts(Etc.getpwuid(pw.uid).name == pw.name)
# Struct-like surface.
puts pw.members.first(4).inspect
puts pw.to_a.length == pw.members.length
puts pw.to_h[:uid] == pw.uid
puts pw[:name] == pw.name
puts pw[0] == pw.name
__END__
Etc::Passwd
true
true
true
true
true
true
true
true
[:name, :passwd, :uid, :gid]
true
true
true
true
