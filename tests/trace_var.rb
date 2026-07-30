# `trace_var` runs something on every assignment RUBY makes to a global.
# (`set_trace_func`, the other half of Kernel's tracing surface, is not here:
# it hands its hook a Binding at each event, which this runtime has no way to
# build from a trace point.)
$seen = []
trace_var(:$watched) { |v| $seen << v }
$watched = 1
$watched = 2
p $seen

# `untrace_var` answers what it dropped, and the hook stops.
p untrace_var(:$watched).size
$watched = 3
p $seen

# Several hooks on one name run NEWEST FIRST, and either form registers one:
# a block, or a command object.
$out = []
command = proc { |v| $out << [:command, v] }
trace_var(:$several, command)
trace_var(:$several) { |v| $out << [:block, v] }
$several = :first
p $out

# Naming the command drops just that one.
p untrace_var(:$several, command).size
$out.clear
$several = :second
p $out

# A hook may assign the variable it watches without re-entering itself.
$clamped = 0
trace_var(:$clamped) { |v| $clamped = 10 if v > 10 }
$clamped = 5
p $clamped
$clamped = 99
p $clamped

# A String names the same variable a Symbol does.
$out.clear
trace_var("$by_string") { |v| $out << v }
$by_string = :ok
p $out

# `trace_var` answers nil; a name nothing ever assigns simply never fires.
p(trace_var(:$never_assigned) { |v| raise "unreachable" })
p untrace_var(:$never_assigned).size

# Without a command and without a block there is nothing to register.
begin
  trace_var(:$no_command)
rescue ArgumentError => e
  p [:no_command, e.message]
end

# `untrace_var` on a name that was neither assigned nor traced is a NameError.
begin
  untrace_var(:$unknown_entirely)
rescue NameError => e
  p [:unknown, e.message]
end

# The runtime's own seeding is not an assignment, so tracing a predefined
# global sees only what the PROGRAM writes to it.
$out.clear
trace_var(:$;) { |v| $out << v }
$; = ","
p $out
untrace_var(:$;)
