require "etc"
puts Etc::SC_CLK_TCK.is_a?(Integer)
puts Etc::SC_OPEN_MAX.is_a?(Integer)
puts Etc::SC_NPROCESSORS_ONLN.is_a?(Integer)
puts Etc::PC_NAME_MAX.is_a?(Integer)
puts Etc::CS_PATH.is_a?(Integer)
puts Etc.sysconf(Etc::SC_CLK_TCK) > 0
puts Etc.sysconf(Etc::SC_OPEN_MAX) > 0
puts(Etc.confstr(Etc::CS_PATH).is_a?(String) || Etc.confstr(Etc::CS_PATH).nil?)
# nprocessors agrees with the sysconf value.
puts(Etc.nprocessors == Etc.sysconf(Etc::SC_NPROCESSORS_ONLN))
__END__
true
true
true
true
true
true
true
true
true
