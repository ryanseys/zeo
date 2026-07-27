# readline is a CRuby C extension (GNU Readline/libedit binding for
# interactive line editing) that zeo has no native implementation of --
# `require "readline"` raises LoadError.
require "readline"
p Readline.respond_to?(:readline)
