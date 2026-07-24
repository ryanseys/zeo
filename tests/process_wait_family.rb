# Process.wait / waitpid / wait2 / waitpid2 / waitall / detach / last_status,
# plus the $? Process::Status they publish and the Errno::ECHILD raised when no
# children remain. Children are spawned with Process.fork and each exits with a
# known code; the output is written relationally so it stays byte-identical to
# CRuby despite the varying pids.

# wait answers the reaped pid and sets $? (== Process.last_status).
pid = Process.fork { exit 7 }
rpid = Process.wait(pid)
puts "wait: #{rpid == pid} status=#{$?.exitstatus} last=#{Process.last_status.exitstatus}"

# waitpid is the same reap-one-child operation.
pid = Process.fork { exit 4 }
rpid = Process.waitpid(pid)
puts "waitpid: #{rpid == pid} status=#{$?.exitstatus}"

# wait2 / waitpid2 answer the [pid, status] pair.
pid = Process.fork { exit 3 }
rp, st = Process.wait2
puts "wait2: #{rp == pid} exit=#{st.exitstatus} success=#{st.success?}"

pid = Process.fork { exit 5 }
rp, st = Process.waitpid2(pid)
puts "waitpid2: #{rp == pid} exit=#{st.exitstatus}"

# waitall reaps every remaining child, newest-status wins $?.
3.times { |i| Process.fork { exit i } }
results = Process.waitall
codes = results.map { |(_p, s)| s.exitstatus }.sort
puts "waitall: count=#{results.length} codes=#{codes.inspect}"

# detach reaps in the background; the returned thread's value is the Status.
t = Process.detach(Process.fork { exit 9 })
puts "detach: value_exit=#{t.value.exitstatus}"

# With no children left, wait raises Errno::ECHILD.
begin
  Process.wait
  puts "echild: NO-RAISE"
rescue Errno::ECHILD
  puts "echild: raised"
end
