# Pull in the statically linked native half FIRST, so `Syslog` and its
# constant table exist to be reopened below. This is CRuby's loader idiom --
# see `gems/strscan/lib/strscan.rb` for the same shape and the reason for it.
require "syslog.so"

module Syslog
  # CRuby defines these submodules in C (`ext/syslog/syslog.c`). Every value
  # here reads back off the native module's own constants, so the two halves
  # cannot drift.

  # The priority-mask macros. Including this (directly, or via Constants)
  # also extends the base, CRuby's included-hook behaviour, so both
  # `LOG_MASK(x)` in an instance method and `Base.LOG_MASK(x)` work.
  module Macros
    def LOG_MASK(pri)
      1 << pri
    end

    def LOG_UPTO(pri)
      (1 << (pri + 1)) - 1
    end

    def self.included(base)
      base.extend(self)
    end
  end

  module Level
    LOG_EMERG = Syslog::LOG_EMERG
    LOG_ALERT = Syslog::LOG_ALERT
    LOG_CRIT = Syslog::LOG_CRIT
    LOG_ERR = Syslog::LOG_ERR
    LOG_WARNING = Syslog::LOG_WARNING
    LOG_NOTICE = Syslog::LOG_NOTICE
    LOG_INFO = Syslog::LOG_INFO
    LOG_DEBUG = Syslog::LOG_DEBUG
  end

  module Option
    LOG_PID = Syslog::LOG_PID
    LOG_CONS = Syslog::LOG_CONS
    LOG_ODELAY = Syslog::LOG_ODELAY
    LOG_NDELAY = Syslog::LOG_NDELAY
    LOG_NOWAIT = Syslog::LOG_NOWAIT
    LOG_PERROR = Syslog::LOG_PERROR
  end

  module Facility
    LOG_KERN = Syslog::LOG_KERN
    LOG_USER = Syslog::LOG_USER
    LOG_MAIL = Syslog::LOG_MAIL
    LOG_DAEMON = Syslog::LOG_DAEMON
    LOG_AUTH = Syslog::LOG_AUTH
    LOG_SYSLOG = Syslog::LOG_SYSLOG
    LOG_LPR = Syslog::LOG_LPR
    LOG_NEWS = Syslog::LOG_NEWS
    LOG_UUCP = Syslog::LOG_UUCP
    LOG_CRON = Syslog::LOG_CRON
    LOG_AUTHPRIV = Syslog::LOG_AUTHPRIV
    LOG_FTP = Syslog::LOG_FTP
    LOG_LOCAL0 = Syslog::LOG_LOCAL0
    LOG_LOCAL1 = Syslog::LOG_LOCAL1
    LOG_LOCAL2 = Syslog::LOG_LOCAL2
    LOG_LOCAL3 = Syslog::LOG_LOCAL3
    LOG_LOCAL4 = Syslog::LOG_LOCAL4
    LOG_LOCAL5 = Syslog::LOG_LOCAL5
    LOG_LOCAL6 = Syslog::LOG_LOCAL6
    LOG_LOCAL7 = Syslog::LOG_LOCAL7
  end

  # Everything at once -- what `include Syslog::Constants` pulls in, macros
  # included (its hook extends the base with them too, as CRuby's does).
  module Constants
    include Macros

    Level.constants.each { |c| const_set(c, Level.const_get(c)) }
    Option.constants.each { |c| const_set(c, Option.const_get(c)) }
    Facility.constants.each { |c| const_set(c, Facility.const_get(c)) }

    def self.included(base)
      base.extend(Macros)
    end
  end
end
