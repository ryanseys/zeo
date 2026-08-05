# Kernel#warn with `uplevel:` prefixes the message with the caller frame's
# "file:line: warning: ". zeo's warn ignores the option and prints the bare
# message.
def helper
  warn "inside helper", uplevel: 0
  warn "callers fault", uplevel: 1
end

helper
warn "no uplevel control"
