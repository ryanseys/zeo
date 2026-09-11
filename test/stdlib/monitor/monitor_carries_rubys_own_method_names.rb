# The names ruby's monitor gives a Monitor and the mixin it comes from.
require "monitor"
p Monitor.instance_methods(false).sort
p MonitorMixin.instance_methods(false).sort
m = Monitor.new
p m.try_mon_enter
p m.mon_owned?
m.mon_exit
p m.mon_owned?
__END__
[:enter, :exit, :mon_check_owner, :mon_enter, :mon_exit, :mon_locked?, :mon_owned?, :mon_synchronize, :mon_try_enter, :new_cond, :synchronize, :try_enter, :try_mon_enter, :wait_for_cond]
[:mon_enter, :mon_exit, :mon_locked?, :mon_owned?, :mon_synchronize, :mon_try_enter, :new_cond, :synchronize, :try_mon_enter]
true
true
false
