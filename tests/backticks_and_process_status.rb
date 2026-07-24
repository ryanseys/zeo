# Backtick command literals, `system`, and `$?` as a `Process::Status`.
# `` `cmd` `` lowers to an implicit-self fcall to the overridable `Kernel#\``,
# capturing the child's stdout; `system` inherits stdio and answers a
# true/false verdict. Both leave the child's wait status in `$?`.

out = `echo hello`
print out
puts out.length
puts $?.exitstatus
puts $?.success?
puts $?.class

p system("true")
puts $?.exitstatus
p system("false")
puts $?.success?
puts $?.exitstatus

# Interpolation uses the normal string path; a `;` forces the shell.
name = "zeo"
print `echo hi #{name}`
print `echo a; echo b`
puts $?.exited?
