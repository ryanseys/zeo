# Thread.current / .main / .pass
p Thread.current.class
p Thread.current == Thread.main
p Thread.pass

# name, value, status
t = Thread.new { 20 + 22 }
t.name = "worker"
p t.name
p t.value          # joins, returns the block's result
p t.status         # false once finished cleanly
p Thread.main.status

# fiber-local storage (Thread#[]) and thread variables are distinct
Thread.current[:tag] = "root"
p Thread.current[:tag]
p Thread.current.key?(:tag)
# (The oracle runs under bundler/setup, whose logger leaves a fiber-local
# key of its own behind; it is not this program's.)
p Thread.current.keys.grep_v(/logger/)
Thread.current.thread_variable_set(:count, 3)
p Thread.current.thread_variable_get(:count)
p Thread.current.thread_variable?(:count)
p Thread.current.thread_variables.grep_v(/logger/)

# report_on_exception defaults true and is settable
p Thread.current.report_on_exception
Thread.current.report_on_exception = false
p Thread.current.report_on_exception

# Poly receiver: a Thread held in an Array dispatches the same rows.
workers = [Thread.new { 1 }, Thread.new { 2 }]
workers.each(&:join)
p workers.map(&:status)
__END__
Thread
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
true
[:count]
true
false
[false, false]
