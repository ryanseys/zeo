# `Module#included` never fires: zeo's include wires the module's methods in
# without invoking the hook, so the extend-on-include idiom (used by
# Syslog::Constants, ActiveSupport::Concern, and countless DSLs) leaves the
# base without the module's class-side methods. `Module#extended` and
# `Class#inherited` are the same mechanism and presumably the same gap.
module Hooked
  def hi
    "instance"
  end

  def self.included(base)
    puts "included fired for #{base}"
    base.extend(self)
  end
end

class Host
  include Hooked
end

p Host.new.hi
p Host.hi
