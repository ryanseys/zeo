require "etc"
u = Etc.uname
puts u.class
puts u.keys.sort.inspect
puts u.values.all? { |v| v.is_a?(String) }
puts u[:sysname].empty? == false
__END__
Hash
[:machine, :nodename, :release, :sysname, :version]
true
true
