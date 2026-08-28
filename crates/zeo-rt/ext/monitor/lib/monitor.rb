require "monitor.so"

# `include MonitorMixin` / `extend MonitorMixin` gives an ordinary object the
# Monitor surface, delegating to a Monitor the way CRuby's lib/monitor.rb
# delegates to @mon_data.
#
# Divergence from CRuby, deliberate: CRuby's MonitorMixin defines
# `initialize(*args)` calling `super` then `mon_initialize`, so including it
# imposes an initialization-order contract on every host class. Here the
# monitor is created LAZILY on first use instead, so `include MonitorMixin`
# needs no cooperation from the host's `initialize` at all. CRuby already uses
# exactly this lazy shape in `new_cond` (`unless defined?(@mon_data);
# mon_initialize; end`), so it is a precedented pattern rather than an
# invention. `mon_initialize` remains available for code that calls it
# explicitly.
#
# Not provided: `new_cond` -- it returns a Monitor::ConditionVariable, which
# the native half does not expose. Absent rather than stubbed, so reaching for
# it is a NoMethodError at the call, not a silent no-op at wait time.
module MonitorMixin
  def mon_initialize
    @mon_data = Monitor.new
    self
  end

  def mon_enter
    __mon_data.enter
  end

  def mon_exit
    __mon_data.exit
  end

  def mon_try_enter
    __mon_data.try_enter
  end

  def mon_locked?
    __mon_data.mon_locked?
  end

  def mon_owned?
    __mon_data.mon_owned?
  end

  def mon_synchronize(&block)
    __mon_data.synchronize(&block)
  end
  alias synchronize mon_synchronize

  private

  def __mon_data
    @mon_data = Monitor.new if @mon_data.nil?
    @mon_data
  end
end
