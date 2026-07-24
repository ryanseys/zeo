# Process.spawn / Process.exec: spawn starts a child WITHOUT waiting (answering
# a reapable pid), exec replaces the current image. Output is written from exit
# statuses only, so it stays byte-identical to CRuby with no temp files.

# spawn answers a pid; Process.wait reaps it and sets $?.
pid = Process.spawn("true")
puts "pid?=#{pid.is_a?(Integer) && pid > 0}"
Process.wait(pid)
puts "true=#{$?.exitstatus}"

pid = Process.spawn("false")
Process.wait(pid)
puts "false=#{$?.exitstatus}"

# Multiple arguments exec directly (no shell); the shell here is explicit.
pid = Process.spawn("sh", "-c", "exit 5")
Process.wait(pid)
puts "sh=#{$?.exitstatus}"

# A leading env hash is passed through to the child.
pid = Process.spawn({ "ZEO_X" => "9" }, "sh", "-c", 'exit $ZEO_X')
Process.wait(pid)
puts "env=#{$?.exitstatus}"

# :unsetenv_others wipes the inherited environment (so ZEO_X is gone unless given).
ENV["ZEO_Y"] = "7"
pid = Process.spawn("sh", "-c", 'exit ${ZEO_Y:-3}', unsetenv_others: true)
Process.wait(pid)
puts "unsetenv=#{$?.exitstatus}"

# exec inside a fork replaces the child image; its exit status flows to $?.
pid = Process.fork { Process.exec("sh", "-c", "exit 4") }
Process.wait(pid)
puts "exec=#{$?.exitstatus}"

# A missing program raises Errno::ENOENT.
begin
  Process.spawn("no_such_command_zxcvb")
  puts "enoent=no"
rescue Errno::ENOENT
  puts "enoent=yes"
end
