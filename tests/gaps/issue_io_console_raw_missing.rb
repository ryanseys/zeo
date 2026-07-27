# io/console is a CRuby C extension. `require "io/console"` loads without a
# LoadError, but the actual console methods (e.g. IO#raw for raw-mode
# terminal input) aren't implemented on IO/STDIN.
require "io/console"
p STDIN.respond_to?(:raw)
