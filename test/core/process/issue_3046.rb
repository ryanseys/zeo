puts Process.getpriority(Process::PRIO_PROCESS, 0).class
g = Process.groups
puts g.class
puts g.all? { |x| x.is_a?(Integer) }
puts [Process::PRIO_PROCESS, Process::PRIO_PGRP, Process::PRIO_USER].inspect
__END__
Integer
Array
true
[0, 1, 2]
