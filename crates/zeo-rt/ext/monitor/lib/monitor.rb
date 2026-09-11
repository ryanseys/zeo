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
module MonitorMixin
  # Upstream's own class, verbatim from ruby/ruby ext/monitor/lib/monitor.rb
  # (the ruby-headers.lock rev): every method is delegation, and the one
  # engine call -- `wait_for_cond` -- is the native half's, exactly as it is
  # CRuby's C half's.
  class ConditionVariable
    def wait(timeout = nil)
      @monitor.mon_check_owner
      @monitor.wait_for_cond(@cond, timeout)
    end

    def wait_while
      while yield
        wait
      end
    end

    def wait_until
      until yield
        wait
      end
    end

    def signal
      @monitor.mon_check_owner
      @cond.signal
    end

    def broadcast
      @monitor.mon_check_owner
      @cond.broadcast
    end

    private

    def initialize(monitor)
      @monitor = monitor
      @cond = Thread::ConditionVariable.new
    end
  end

  def mon_enter
    __mon_data.enter
  end

  def mon_exit
    __mon_data.exit
  end

  # `try_mon_enter` is the older spelling ruby keeps as an alias.
  def try_mon_enter
    mon_try_enter
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

  def new_cond
    ConditionVariable.new(__mon_data)
  end

  private

  def mon_initialize
    @mon_data = Monitor.new
    self
  end

  def mon_check_owner
    __mon_data.mon_check_owner
  end

  def __mon_data
    @mon_data = Monitor.new if @mon_data.nil?
    @mon_data
  end
end

class Monitor
  def new_cond
    ::MonitorMixin::ConditionVariable.new(self)
  end
end
