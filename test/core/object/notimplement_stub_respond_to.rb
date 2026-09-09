# `respond_to?` answered TRUE for a method ruby installs as a not-implemented
# stub, where ruby answers false.
#
# CRuby defines the calls a platform lacks (`Kernel#syscall` on macOS,
# `Process::Sys.setresuid` where the syscall is absent) with
# `rb_f_notimplement`, and tells such an entry apart by its implementation
# IDENTITY. The entry is still LISTED -- `private_instance_methods` and
# `singleton_methods` carry it, and `instance_method(:syscall).arity` answers
# 0 -- but `respond_to?` reports false, which is how a program is meant to
# detect the absence before calling.
#
# zeo names the rows instead (`dispatch::is_notimplement_row`). The question is
# asked at `respond_to?`'s own entry rather than in `responds_to`, which doubles
# as an EXISTENCE predicate: a stub does exist, so `instance_method` must still
# build an `UnboundMethod` for it.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e
  puts "#{label}: #{e.class}: #{e.message}"
end

STUBS = [
  ["Kernel#syscall", -> { syscall(999_999) }],
  ["Process::Sys.setresuid", -> { Process::Sys.setresuid(0, 0, 0) }],
  ["Process::Sys.setresgid", -> { Process::Sys.setresgid(0, 0, 0) }]
].freeze

# `respond_to?` says no, in both the instance and the singleton form.
show("respond_to? syscall") { respond_to?(:syscall, true) }
show("respond_to? syscall public") { respond_to?(:syscall) }
show("Process::Sys.respond_to? setresuid") { Process::Sys.respond_to?(:setresuid) }
show("Process::Sys.respond_to? setresgid") { Process::Sys.respond_to?(:setresgid) }

# ...while every listing still carries them, which is what makes this a missing
# BIT rather than a missing method.
show("listed as private") { Kernel.private_instance_methods(false).include?(:syscall) }
show("listed as singleton") { Kernel.singleton_methods(false).include?(:syscall) }
show("private_method_defined?") { Kernel.private_method_defined?(:syscall) }
show("arity") { Kernel.instance_method(:syscall).arity }
show("owner") { Kernel.instance_method(:syscall).owner }
show("Sys singleton listing") { Process::Sys.singleton_methods(false).include?(:setresuid) }

# A name that is NOT a stub is unaffected, on the same modules.
show("respond_to? puts") { respond_to?(:puts, true) }
show("Process::Sys.respond_to? getuid") { Process::Sys.respond_to?(:getuid) }

# Every listed stub must actually refuse -- the check that stops the table in
# `is_notimplement_row` from going stale by having a body implemented out from
# under it.
STUBS.each do |name, call|
  call.call
  puts "#{name}: DID NOT RAISE"
rescue NotImplementedError => e
  puts "#{name}: #{e.class}: #{e.message}"
end
__END__
respond_to? syscall: false
respond_to? syscall public: false
Process::Sys.respond_to? setresuid: false
Process::Sys.respond_to? setresgid: false
listed as private: true
listed as singleton: true
private_method_defined?: false
arity: 0
owner: Kernel
Sys singleton listing: true
respond_to? puts: true
Process::Sys.respond_to? getuid: true
Kernel#syscall: NotImplementedError: syscall() function is unimplemented on this machine
Process::Sys.setresuid: NotImplementedError: setresuid() function is unimplemented on this machine
Process::Sys.setresgid: NotImplementedError: setresgid() function is unimplemented on this machine
