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

# Moved out of the spinel mirror: the corpus claimed a bare top-level `@@x`
# stores into the same namespace. It doesn't -- only a `class`/`module` body
# opens the cref `@@x` resolves against, so Ruby raises here. See
# `class_variable_cref_scope.rb` for the full rule.
begin
  @@plain = 42
rescue RuntimeError => e
  puts e.message                  # class variable access from toplevel
end
