# Etc: the system user/group databases, sysconf/confstr, uname, and processor
# count -- CRuby's native `ext/etc` over libc. Host-specific values (the real
# uid, hostname, cpu count) can't be printed verbatim, so this prints the
# CONTRACT -- types, round-trips, and member sets -- identical under zeo and ruby.
require "etc"

# The current user, and a name/uid round-trip through the database.
pw = Etc.getpwuid
puts pw.class
puts pw.name.is_a?(String)
puts pw.uid.is_a?(Integer)
puts(Etc.getpwnam(pw.name).uid == pw.uid)
puts(Etc.getpwuid(pw.uid).name == pw.name)

# Etc::Passwd is Struct-like.
puts pw.members.first(4).inspect
puts pw.to_a.length == pw.members.length
puts pw.to_h[:uid] == pw.uid
puts pw[:name] == pw.name

# The group database, mirroring the user one.
gr = Etc.getgrgid
puts gr.class
puts gr.mem.is_a?(Array)
puts(Etc.getgrnam(gr.name).gid == gr.gid)

# uname(2): the five portable system fields.
u = Etc.uname
puts u.keys.sort.inspect
puts u.values.all? { |v| v.is_a?(String) }

# Runtime configuration + processor count.
puts Etc.nprocessors > 0
puts(Etc.nprocessors == Etc.sysconf(Etc::SC_NPROCESSORS_ONLN))
puts Etc.sysconf(Etc::SC_CLK_TCK) > 0
puts Etc::SC_OPEN_MAX.is_a?(Integer)
puts Etc::PC_NAME_MAX.is_a?(Integer)

# The whole database iterates, and always includes root (uid 0).
saw_root = false
Etc.passwd { |p| saw_root = true if p.uid == 0 }
puts saw_root
__END__
Etc::Passwd
true
true
true
true
[:name, :passwd, :uid, :gid]
true
true
true
Etc::Group
true
true
[:machine, :nodename, :release, :sysname, :version]
true
true
true
true
true
true
true
