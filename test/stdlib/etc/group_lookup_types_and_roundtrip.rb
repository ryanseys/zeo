require "etc"
gr = Etc.getgrgid           # current effective gid
puts gr.class
puts gr.is_a?(Etc::Group)
puts gr.name.is_a?(String)
puts gr.gid.is_a?(Integer)
puts gr.mem.is_a?(Array)
puts(Etc.getgrnam(gr.name).gid == gr.gid)
puts gr.members.inspect
puts gr.to_h.key?(:mem)
__END__
Etc::Group
true
true
true
true
true
[:name, :passwd, :gid, :mem]
true
