# syslog is a CRuby C extension (binding to the system syslog(3) facility)
# that zeo has no native implementation of -- `require "syslog"` raises
# LoadError.
require "syslog"
p Syslog.respond_to?(:open)
