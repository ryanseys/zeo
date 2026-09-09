# zeo registers a builtin's require-gated rows unconditionally, so a method
# that ruby only grows after `require "io/console"` (or `"io/nonblock"`)
# answers `respond_to?` and appears in `instance_methods(false)` in a program
# that never required it.
#
# The rows come from `ruby_class!` tables compiled into the runtime, and the
# table is registered with the class. There is a mechanism for exactly this
# -- `REG_REGISTER_BUILTIN` exists so a require-gated builtin (StringIO, Set,
# Digest, JSON) registers at its `require` -- and the `io/console` and
# `io/nonblock` rows simply do not use it: they are defined on `IO` itself
# rather than as a gated overlay.
#
# 36 rows on `IO` today. It matters beyond reflection because
# `respond_to?(:getch)` is how a library decides whether the console
# extension is available, and zeo answers yes before the require.
#
# `IO#to_s` is a different bug in the same list: ruby inherits it from
# `Object` and zeo declares it as IO's own, so it shows up in
# `instance_methods(false)`. And `pathconf` is MISSING, which is the
# ordinary direction and belongs to the surface comparison.
#
# The surface comparison gates the "zeo is missing a row" direction only;
# this EXTRA direction is ungated.
#
# Oracle: none of these rows exist until the library is required.
p IO.instance_methods(false).include?(:getch)
p IO.instance_methods(false).include?(:nonblock)
p IO.instance_methods(false).include?(:to_s)
p STDOUT.respond_to?(:winsize)
p STDOUT.respond_to?(:cooked)
__END__
false
false
false
false
false
