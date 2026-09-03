#@ only: macos
# `Process::Sys`, `Process::UID` and `Process::GID` -- the identity surface.
#
# Every id printed here is compared against one the process already knows,
# never printed raw, so the output is the same on any machine. Nothing in
# this file can change an id: an unprivileged process may only re-assert the
# ids it already holds, and every other request is refused.

uid = Process.uid
euid = Process.euid
gid = Process.gid
egid = Process.egid

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

# The shape: all three are modules, and every name but the two `eid=`
# writers is a `module_function` -- a singleton method AND a private
# instance method, so `include Process::Sys` carries them too.
[Process::Sys, Process::UID, Process::GID].each do |m|
  puts "#{m} class: #{m.class}"
  puts "#{m} singleton: #{m.singleton_methods(false).sort.inspect}"
  puts "#{m} private: #{m.private_instance_methods(false).sort.inspect}"
  puts "#{m} public instance: #{m.instance_methods(false).inspect}"
end
puts "Sys respond_to?: #{Process::Sys.respond_to?(:setuid)}"
puts "Sys included: #{Class.new { include Process::Sys }.new.send(:getuid) == uid}"

# The readers, each against the answer `Process` already gives.
show("Sys.getuid") { Process::Sys.getuid == uid }
show("Sys.geteuid") { Process::Sys.geteuid == euid }
show("Sys.getgid") { Process::Sys.getgid == gid }
show("Sys.getegid") { Process::Sys.getegid == egid }
show("Sys.issetugid") { Process::Sys.issetugid }
show("UID.rid") { Process::UID.rid == uid }
show("UID.eid") { Process::UID.eid == euid }
show("GID.rid") { Process::GID.rid == gid }
show("GID.eid") { Process::GID.eid == egid }
show("UID.sid_available?") { Process::UID.sid_available? }
show("GID.sid_available?") { Process::GID.sid_available? }

# `from_name` resolves a passwd or group name; an Integer passes straight
# through, and anything that is neither takes the Integer conversion's own
# refusal.
show("UID.from_name root") { Process::UID.from_name("root") }
show("UID.from_name self") { Process::UID.from_name(uid) == uid }
show("GID.from_name self") { Process::GID.from_name(gid) == gid }
show("UID.from_name float") { Process::UID.from_name(1.9) }
show("UID.from_name negative") { Process::UID.from_name(-1) }
show("UID.from_name to_int") do
  o = Object.new
  def o.to_int = 7
  Process::UID.from_name(o)
end
show("UID.from_name bignum") { Process::UID.from_name(2**70) }
show("UID.from_name nil") { Process::UID.from_name(nil) }
show("UID.from_name symbol") { Process::UID.from_name(:root) }
show("UID.from_name missing") { Process::UID.from_name("nosuchuser__") }
show("GID.from_name missing") { Process::GID.from_name("nosuchgroup__") }

# Re-asserting the ids the process already holds is the one change it is
# allowed to make, and it answers what CRuby answers: nil from `Sys`, the
# argument as written from `UID`/`GID`.
show("Sys.setuid self") { Process::Sys.setuid(uid) }
show("Sys.setgid self") { Process::Sys.setgid(gid) }
show("Sys.seteuid self") { Process::Sys.seteuid(euid) }
show("Sys.setegid self") { Process::Sys.setegid(egid) }
show("Sys.setruid self") { Process::Sys.setruid(uid) }
show("Sys.setrgid self") { Process::Sys.setrgid(gid) }
show("Sys.setreuid self") { Process::Sys.setreuid(uid, euid) }
show("Sys.setregid self") { Process::Sys.setregid(gid, egid) }
show("UID.change_privilege self") { Process::UID.change_privilege(uid) == uid }
show("GID.change_privilege self") { Process::GID.change_privilege(gid) == gid }
show("UID.grant_privilege self") { Process::UID.grant_privilege(uid) == uid }
show("GID.grant_privilege self") { Process::GID.grant_privilege(gid) == gid }
show("UID.eid= self") { (Process::UID.eid = uid) == uid }
show("GID.eid= self") { (Process::GID.eid = gid) == gid }

# Every request for an id this process does not hold is refused, and by the
# BARE errno message -- CRuby's `rb_sys_fail(0)` appends no context.
show("Sys.setuid root") { Process::Sys.setuid(0) }
show("Sys.setgid wheel") { Process::Sys.setgid(0) }
show("Sys.seteuid root") { Process::Sys.seteuid(0) }
show("Sys.setreuid root") { Process::Sys.setreuid(0, 0) }
show("UID.change_privilege root") { Process::UID.change_privilege(0) }
show("UID.grant_privilege root") { Process::UID.grant_privilege(0) }
show("UID.eid= root") { Process::UID.eid = 0 }
show("UID.switch") { Process::UID.switch }
show("UID.switch block") { Process::UID.switch { 1 } }
show("GID.switch") { Process::GID.switch }
# A name that resolves is still refused; a name that does not never gets
# as far as the syscall.
show("Sys.setuid by name") { Process::Sys.setuid("root") }
show("Sys.setuid bad name") { Process::Sys.setuid("nosuchuser__") }
show("Sys.setgid bad name") { Process::Sys.setgid("nosuchgroup__") }

# `setresuid`/`setresgid` are the not-implemented stubs, which take any
# arguments at all and still report arity 0.
show("Sys.setresuid") { Process::Sys.setresuid(0, 0, 0) }
show("Sys.setresgid") { Process::Sys.setresgid }
puts "setresuid arity: #{Process::Sys.method(:setresuid).arity}"

# `re_exchangeable?` answers for the platform, so the golden checks that
# `re_exchange` AGREES with it rather than printing either answer.
[Process::UID, Process::GID].each do |m|
  swappable = m.re_exchangeable?
  agreed = begin
    m.re_exchange
    swappable
  rescue NotImplementedError
    !swappable
  rescue SystemCallError
    swappable
  end
  puts "#{m} re_exchange agrees: #{agreed}"
end
__END__
Process::Sys class: Module
Process::Sys singleton: [:getegid, :geteuid, :getgid, :getuid, :issetugid, :setegid, :seteuid, :setgid, :setregid, :setresgid, :setresuid, :setreuid, :setrgid, :setruid, :setuid]
Process::Sys private: [:getegid, :geteuid, :getgid, :getuid, :issetugid, :setegid, :seteuid, :setgid, :setregid, :setresgid, :setresuid, :setreuid, :setrgid, :setruid, :setuid]
Process::Sys public instance: []
Process::UID class: Module
Process::UID singleton: [:change_privilege, :eid, :eid=, :from_name, :grant_privilege, :re_exchange, :re_exchangeable?, :rid, :sid_available?, :switch]
Process::UID private: [:change_privilege, :eid, :from_name, :grant_privilege, :re_exchange, :re_exchangeable?, :rid, :sid_available?, :switch]
Process::UID public instance: []
Process::GID class: Module
Process::GID singleton: [:change_privilege, :eid, :eid=, :from_name, :grant_privilege, :re_exchange, :re_exchangeable?, :rid, :sid_available?, :switch]
Process::GID private: [:change_privilege, :eid, :from_name, :grant_privilege, :re_exchange, :re_exchangeable?, :rid, :sid_available?, :switch]
Process::GID public instance: []
Sys respond_to?: true
Sys included: true
Sys.getuid: true
Sys.geteuid: true
Sys.getgid: true
Sys.getegid: true
Sys.issetugid: false
UID.rid: true
UID.eid: true
GID.rid: true
GID.eid: true
UID.sid_available?: true
GID.sid_available?: true
UID.from_name root: 0
UID.from_name self: true
GID.from_name self: true
UID.from_name float: 1
UID.from_name negative: 4294967295
UID.from_name to_int: 7
UID.from_name bignum: RangeError: bignum too big to convert into 'unsigned long'
UID.from_name nil: TypeError: no implicit conversion of nil into Integer
UID.from_name symbol: TypeError: no implicit conversion of Symbol into Integer
UID.from_name missing: ArgumentError: can't find user for nosuchuser__
GID.from_name missing: ArgumentError: can't find group for nosuchgroup__
Sys.setuid self: nil
Sys.setgid self: nil
Sys.seteuid self: nil
Sys.setegid self: nil
Sys.setruid self: nil
Sys.setrgid self: nil
Sys.setreuid self: nil
Sys.setregid self: nil
UID.change_privilege self: true
GID.change_privilege self: true
UID.grant_privilege self: true
GID.grant_privilege self: true
UID.eid= self: true
GID.eid= self: true
Sys.setuid root: Errno::EPERM: Operation not permitted
Sys.setgid wheel: Errno::EPERM: Operation not permitted
Sys.seteuid root: Errno::EPERM: Operation not permitted
Sys.setreuid root: Errno::EPERM: Operation not permitted
UID.change_privilege root: Errno::EPERM: Operation not permitted
UID.grant_privilege root: Errno::EPERM: Operation not permitted
UID.eid= root: Errno::EPERM: Operation not permitted
UID.switch: Errno::EPERM: Operation not permitted
UID.switch block: Errno::EPERM: Operation not permitted
GID.switch: Errno::EPERM: Operation not permitted
Sys.setuid by name: Errno::EPERM: Operation not permitted
Sys.setuid bad name: ArgumentError: can't find user for nosuchuser__
Sys.setgid bad name: ArgumentError: can't find group for nosuchgroup__
Sys.setresuid: NotImplementedError: setresuid() function is unimplemented on this machine
Sys.setresgid: NotImplementedError: setresgid() function is unimplemented on this machine
setresuid arity: 0
Process::UID re_exchange agrees: true
Process::GID re_exchange agrees: true
