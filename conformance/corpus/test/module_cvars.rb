# Regression: class variables (`@@var`) declared at module scope or
# top-level scope generate references to a `cvar_Toplevel_X` C
# global, but the declaration walker (`collect_cvars`) used to only
# descend into ClassNode bodies. The result: `cvar_Toplevel_X` was
# referenced but never declared, breaking the C compile.

module Tep
  @@session_secret = ""

  def self.session_secret
    @@session_secret
  end

  def self.session_secret=(v)
    @@session_secret = v
  end
end

# Read default
puts Tep.session_secret.length    # 0

# Write + read back
Tep.session_secret = "hello"
puts Tep.session_secret           # hello

# Read again to confirm the global persists
puts Tep.session_secret           # hello

# Bare top-level `@@x` (legal in Ruby though unusual) -- same
# Toplevel namespace, just no enclosing module/class.
@@plain = 42
puts @@plain                      # 42
