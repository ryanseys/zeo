# A top-level `def` written in a box belongs to that box: main cannot call
# it.
#
# A top-level `def` is a private instance method of `Object`, and a box has
# its OWN `Object` -- so a `def` in a box means the `class Object` reopen
# that a box already isolates (a per-box builtin OVERLAY whose rows
# register on the root's entry keyed by the box). Analyze wraps it into
# exactly that; without the wrap the def fell through to the run-time
# install, which knows one `Object`.
#
# The Object root had to become resolvable from a box for that to work. It
# predates the builtin placeholders and carries neither `is_builtin` nor
# `is_bootstrap`, so a box's name lookup skipped it and minted a SECOND
# class called `Object` instead of an overlay of the real one.

b = Ruby::Box.new
b.eval("def leaked_helper = 1")
begin
  p leaked_helper
rescue NameError, NoMethodError => e
  p e.class
end
