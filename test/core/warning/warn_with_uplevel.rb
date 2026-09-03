# Kernel#warn with `uplevel:` prefixes the message with the caller frame's
# "file:line: warning: ". zeo's warn ignores the option and prints the bare
# message.
def helper
  warn "inside helper", uplevel: 0
  warn "callers fault", uplevel: 1
end

helper
warn "no uplevel control"
__END__
#@ stderr
core/warning/warn_with_uplevel.rb:5: warning: inside helper
core/warning/warn_with_uplevel.rb:9: warning: callers fault
no uplevel control
