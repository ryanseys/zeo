# pty is a CRuby C extension (pseudo-terminal support) that zeo has no
# native implementation of -- `require "pty"` raises LoadError.
require "pty"
p PTY.respond_to?(:spawn)
