p Thread.current == Thread.main
p Thread.pass
t = Thread.new { 20 + 22 }
t.name = "worker"
p t.name
p t.value
p t.status
p Thread.main.status
Thread.current[:tag] = "root"
p Thread.current[:tag]
p Thread.current.key?(:tag)
p Thread.current.keys
Thread.current.thread_variable_set(:count, 3)
p Thread.current.thread_variable_get(:count)
p Thread.current.thread_variables.grep_v(/logger/)
p Thread.current.report_on_exception
workers = [Thread.new { 1 }, Thread.new { 2 }]
workers.each(&:join)
p workers.map(&:status)
__END__
true
nil
"worker"
42
false
"run"
"root"
true
[:tag]
3
[:count]
true
[false, false]
