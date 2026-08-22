# `Ruby::Box`'s gate-only rows are present even when boxes are DISABLED.
#
# CRuby defines `Ruby::Box.main`/`.root`/`.master` and `#main?`/`#root?`/
# `#master?` only under `RUBY_BOX=1`; without it the class carries six
# fewer methods, and calling one is a `NoMethodError`. zeo's rows live in
# a static table the `ruby_class!` DSL builds at compile time, so they are
# registered either way and answer the gate's own `RuntimeError` instead.
#
# Nothing a program does can reach a WRONG answer through this -- every
# one of the six refuses when the gate is off, and the refusal names the
# gate. What differs is the REFLECTION: `instance_methods(false)` and
# `singleton_methods(false)` list six names CRuby does not.
#
# The fix shape: a run-time undef of the six at bootstrap when the gate is
# off. The instance half already has a mechanism (`mark_undefined`); the
# class half has none, which is why this is written down rather than
# fixed in passing.
#
# Oracle: the disabled surface is the smaller one.
p (Ruby::Box.singleton_methods(false) - %i[current enabled?]).sort
p (Ruby::Box.instance_methods(false) -
   %i[eval inspect load load_path require require_relative]).sort
begin
  Ruby::Box.main
rescue Exception => e
  p e.class
end
