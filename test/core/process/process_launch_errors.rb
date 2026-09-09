# Process-launch error shapes: `system(cmd, exception: true)` raises
# RuntimeError on failure and Errno::ENOENT on a missing command (zeo
# answers false/nil, ignoring the option), and a bare `spawn` of a
# missing command names it in the ENOENT message ("No such file or
# directory - cmd"); zeo's message is the @-form with an empty site.
def show
  p yield
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end
show { system("false", exception: true) }
show { system("definitely_not_a_cmd_xyz", exception: true) }
show { spawn("definitely_not_a_cmd_xyz") }
__END__
RuntimeError: Command failed with exit 1: false
Errno::ENOENT: No such file or directory - definitely_not_a_cmd_xyz
Errno::ENOENT: No such file or directory - definitely_not_a_cmd_xyz
