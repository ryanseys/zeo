# Process-launch error shapes: `system(cmd, exception: true)` raises
# RuntimeError on failure and Errno::ENOENT on a missing command (zeo
# answers false/nil, ignoring the option), and a bare `spawn` of a
# missing command names it in the ENOENT message ("No such file or
# directory - cmd"); zeo's message is the @-form with an empty site.
# (Found by the 2026-08-24 probe sweep.)
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { system("false", exception: true) }
show { system("definitely_not_a_cmd_xyz", exception: true) }
show { spawn("definitely_not_a_cmd_xyz") }
