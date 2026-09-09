# `module_function` gives a name TWO copies: a public singleton method on the
# module, and a PRIVATE instance method that `include` carries into a class.
# Kernel, Process, FileTest, ObjectSpace and Signal are all built this way in
# CRuby, so `include Process` must make `pid` callable without a receiver.

class Host
  include Process

  def report_pid
    pid
  end

  def report_ppid
    ppid
  end
end

puts "-- the instance half arrives through include"
p Host.new.report_pid == Process.pid
p Host.new.report_ppid == Process.ppid

puts "-- and it is private: reflection says so, and public_send refuses"
p Host.private_method_defined?(:pid)
p Host.new.respond_to?(:pid)
begin
  Host.new.public_send(:pid)
rescue NoMethodError => e
  puts e.message
end
# The plain `Host.new.pid` call is NOT asserted here: codegen folds an
# explicit-receiver builtin call without consulting visibility, so it answers
# instead of raising. See `test/lang/modules/module_function_private_receiver.rb`.

puts "-- the singleton half stays public"
p Process.respond_to?(:pid)
p Process.pid.is_a?(Integer)

puts "-- counts match CRuby's own"
p Process.private_instance_methods(false).size
p Process.singleton_methods(false).size
p Signal.private_instance_methods(false).sort

puts "-- the eight Process names CRuby keeps singleton-only stay singleton-only"
%i[_fork abort exec exit exit! fork last_status spawn].each do |m|
  puts "#{m}: singleton=#{Process.singleton_methods(false).include?(m)} " \
       "private=#{Process.private_instance_methods(false).include?(m)}"
end

puts "-- FileTest mixes in the same 26 predicates File exposes as class methods"

class Prober
  include FileTest

  def tmp_there?
    directory?("/tmp")
  end
end

p Prober.new.tmp_there?
p FileTest.private_instance_methods(false).size
p FileTest.singleton_methods(false).size
p FileTest.exist?("/tmp")
p FileTest.identical?("/tmp", "/tmp")

puts "-- but FileTest is not the whole of File's class surface"
begin
  FileTest.read("/etc/hosts")
rescue NoMethodError => e
  puts e.message
end
p File.read("/etc/hosts").is_a?(String)

puts "-- Kernel's converted rows answer as class methods too"
p Kernel.respond_to?(:format)
p Kernel.format("%05.2f", 1.5)
p Kernel.singleton_methods(false).include?(:sprintf)
__END__
-- the instance half arrives through include
true
true
-- and it is private: reflection says so, and public_send refuses
true
false
private method 'pid' called for an instance of Host
-- the singleton half stays public
true
true
-- counts match CRuby's own
39
47
[:list, :signame, :trap]
-- the eight Process names CRuby keeps singleton-only stay singleton-only
_fork: singleton=true private=false
abort: singleton=true private=false
exec: singleton=true private=false
exit: singleton=true private=false
exit!: singleton=true private=false
fork: singleton=true private=false
last_status: singleton=true private=false
spawn: singleton=true private=false
-- FileTest mixes in the same 26 predicates File exposes as class methods
true
26
26
true
true
-- but FileTest is not the whole of File's class surface
undefined method 'read' for module FileTest
true
-- Kernel's converted rows answer as class methods too
true
"01.50"
true
