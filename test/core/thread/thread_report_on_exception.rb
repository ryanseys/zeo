# A thread that dies of an uncaught exception says so on stderr as it
# terminates, naming itself. That report and the re-raise at `#join` are
# SEPARATE mechanisms -- a program can see both, or silence the report and
# still get the re-raise. See the sidecar golden for what reaches stderr.

# --- reported, AND re-raised at join -----------------------------------------
loud = Thread.new { raise "reported" }
begin
  loud.join
rescue RuntimeError => e
  puts "joined: #{e.message}"
end

# --- silenced per thread: the re-raise survives ------------------------------
# The thread waits for a go-ahead, so the flag is definitely set before it
# can raise -- setting it on an already-running thread is a race.
gate = Queue.new
quiet = Thread.new do
  gate.pop
  raise "silent"
end
quiet.report_on_exception = false
gate.push(:go)
begin
  quiet.value
rescue RuntimeError => e
  puts "joined: #{e.message}"
end
p quiet.report_on_exception

# --- the class-level default seeds threads spawned AFTER it ------------------
p Thread.report_on_exception
Thread.report_on_exception = false
p Thread.report_on_exception
born_quiet = Thread.new { raise "inherited silence" }
p born_quiet.report_on_exception
begin
  born_quiet.join
rescue RuntimeError => e
  puts "joined: #{e.message}"
end

# Back on, and a fresh thread is loud again.
Thread.report_on_exception = true
loud_again = Thread.new { raise "loud again" }
begin
  loud_again.join
rescue RuntimeError => e
  puts "joined: #{e.message}"
end

# --- a killed thread reports nothing: it dies silently by design -------------
# It parks on an empty Queue, the deterministic point a pending kill lands.
Thread.report_on_exception = true
idle = Queue.new
killed = Thread.new { idle.pop }
killed.kill
p killed.join.equal?(killed)
p killed.value

# --- a thread that finishes normally reports nothing -------------------------
Thread.report_on_exception = true
p Thread.new { 1 + 1 }.value

puts "done"
__END__
joined: reported
joined: silent
false
true
false
false
joined: inherited silence
joined: loud again
true
nil
2
done
#@ stderr
#<Thread:0xADDR core/thread/thread_report_on_exception.rb:7 run> terminated with exception (report_on_exception is true):
core/thread/thread_report_on_exception.rb:7:in 'block in <main>': reported (RuntimeError)
#<Thread:0xADDR core/thread/thread_report_on_exception.rb:45 run> terminated with exception (report_on_exception is true):
core/thread/thread_report_on_exception.rb:45:in 'block in <main>': loud again (RuntimeError)
