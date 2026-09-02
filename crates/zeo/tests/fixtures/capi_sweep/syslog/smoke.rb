require "syslog"

Syslog.open("zeo-sweep", Syslog::LOG_PID | Syslog::LOG_NDELAY, Syslog::LOG_USER)
puts Syslog.opened?, Syslog.ident, Syslog.options, Syslog.facility
puts Syslog.mask.class, Syslog::LOG_MASK(Syslog::LOG_ERR), Syslog::LOG_UPTO(Syslog::LOG_WARNING)
Syslog.log(Syslog::LOG_DEBUG, "zeo capi sweep %d", 1)
Syslog.debug("d")
Syslog.info("i")
Syslog.close
puts Syslog.opened?
