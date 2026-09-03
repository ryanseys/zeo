# `require` behaviors that follow from CRuby's load.c, in the two places
# zeo's compile-time resolver used to reject outright.

# 1. A `require` of a feature the runtime already provides natively works from
#    ANY position -- inside a conditional, a method body, or a value position.
#    There is nothing to splice: the whole effect is activating a built-in, and
#    that is a compile-time act. CRuby answers true the first time a feature
#    loads and false thereafter (load.c:1413), so the call has a real value.
first = require "digest"
again = require "digest"
p first
p again

if 1 > 0
  require "json"
end

def load_it
  require "set"
  "ok"
end
p load_it

# 2. Because the require is an ordinary expression now, a trailing modifier
#    attaches to it instead of stranding the parser.
require "base64" if false
require "zlib" rescue nil
puts "modifiers ok"

# 3. `require "time"` and `require "io/console"` name no gated constant here --
#    Time and IO are always-on -- so both are accepted no-ops.
require "time"
p Time.at(0).utc.year

# 4. A reentrant lock from `require "monitor"`. The distinguishing property
#    against Mutex is exactly re-entrancy: the owner may enter again, and the
#    lock only releases at the outermost exit.
require "monitor"
mon = Monitor.new
mon.synchronize do
  mon.synchronize { p mon.mon_owned? }
  p mon.mon_locked?
end
p mon.mon_locked?
p mon.synchronize { 21 * 2 }
__END__
true
false
"ok"
modifiers ok
1970
true
true
false
42
