# syslog -- the system logger. One process-wide connection with CRuby's
# lifecycle: closed accessors answer nil, a second open refuses, the mask
# resets per open, and a message makes the trip as sprintf-then-"%s".
require "syslog"

p Syslog.respond_to?(:open)
p [Syslog.opened?, Syslog.ident, Syslog.options, Syslog.facility, Syslog.mask]
begin
  Syslog.log(Syslog::LOG_INFO, "x")
rescue RuntimeError => e
  p e.message
end

p [Syslog::LOG_PID, Syslog::LOG_CONS, Syslog::LOG_NDELAY, Syslog::LOG_USER, Syslog::LOG_LOCAL3]
p [Syslog::LOG_EMERG, Syslog::LOG_ERR, Syslog::LOG_WARNING, Syslog::LOG_DEBUG]
p [Syslog.LOG_MASK(Syslog::LOG_ERR), Syslog.LOG_UPTO(Syslog::LOG_ERR)]

ret = Syslog.open("zeo_golden")
p ret == Syslog
p [Syslog.opened?, Syslog.ident, Syslog.options, Syslog.facility, Syslog.mask]
p Syslog.inspect
begin
  Syslog.open("again")
rescue RuntimeError => e
  p e.message
end
Syslog.log(Syslog::LOG_DEBUG, "zeo syslog golden %d%% there", 99)
Syslog.debug("shortcut %s", "row")
Syslog.mask = Syslog.LOG_UPTO(Syslog::LOG_WARNING)
p Syslog.mask
p Syslog.instance == Syslog

Syslog.reopen("zeo_golden2", Syslog::LOG_NDELAY, Syslog::LOG_LOCAL3)
p [Syslog.ident, Syslog.options, Syslog.facility]

Syslog.close
p [Syslog.opened?, Syslog.ident]
p Syslog.inspect
begin
  Syslog.close
rescue RuntimeError => e
  p e.message
end

blk = Syslog.open("zeo_blk") { |s| p [s.ident, s.opened?] }
p [blk == Syslog, Syslog.opened?]

# The Ruby half: the constant submodules and the include-hook macros.
p Syslog::Level::LOG_ERR
p Syslog::Option::LOG_PID
p Syslog::Facility::LOG_LOCAL7
p Syslog::Constants::LOG_ERR
# The SHAPE, not the number: zeo tracks syslog's latest upstream release and
# the oracle reads the copy ruby 4.0.6 ships (`crates/zeo-rt/ext/UPSTREAM.md`).
p Syslog::VERSION.match?(/\A\d+\.\d+\.\d+\z/)

class UsesConstants
  include Syslog::Constants

  def upto
    LOG_UPTO(LOG_ERR)
  end
end
p UsesConstants.new.upto
# (`UsesConstants.LOG_MASK` -- the extend-on-include hook -- is
# tests/gaps/issue_included_hook_not_fired.rb.)

# The vendored Syslog::Logger rides on all of the above.
require "syslog/logger"
slog = Syslog::Logger.new("zeo_slogger")
p slog.class
slog.info("via Syslog::Logger")
p slog.level
__END__
true
[false, nil, nil, nil, nil]
"must open syslog before write"
[1, 2, 8, 8, 152]
[0, 3, 4, 7]
[8, 15]
true
[true, "zeo_golden", 3, 8, 255]
"<#Syslog: opened=true, ident=\"zeo_golden\", options=3, facility=8, mask=255>"
"syslog already open"
31
true
["zeo_golden2", 8, 152]
[false, nil]
"<#Syslog: opened=false>"
"syslog not opened"
["zeo_blk", true]
[true, false]
3
1
184
3
true
15
Syslog::Logger
0
