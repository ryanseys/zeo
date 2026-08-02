# `respond_to?` answers TRUE for a method ruby installs as a
# not-implemented stub, where ruby answers false.
#
# CRuby defines the calls a platform lacks (`Kernel#syscall` on macOS,
# `Process::Sys.setresuid` where the syscall is absent) with
# `rb_f_notimplement`. Such an entry is still LISTED --
# `private_instance_methods` and `singleton_methods` both carry it, and
# `instance_method(:syscall).arity` answers 0 -- but `respond_to?` reports
# false, which is how a program is meant to detect the absence before
# calling. zeo's method table carries no "this row is a stub" bit, so
# `respond_to?` sees an ordinary row.
#
# Calling one is right either way: both raise NotImplementedError with the
# same message. Fix shape: a flag on the generated row (the same place a
# third visibility state belongs, see tests/gaps/builtin_protected_rows.rb)
# that `respond_to?` consults and the listings ignore.

def show(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}: #{e.message}"
end

show("respond_to? syscall") { respond_to?(:syscall, true) }
show("respond_to? Process::Sys.setresuid") { Process::Sys.respond_to?(:setresuid) }

# Listed all the same, which is what makes this a missing bit rather than a
# missing method.
show("listed as private") { Kernel.private_instance_methods(false).include?(:syscall) }
show("listed as singleton") { Kernel.singleton_methods(false).include?(:syscall) }
show("arity") { Kernel.instance_method(:syscall).arity }

# And calling it refuses identically.
show("calling syscall") { send(:syscall, 999_999) }
