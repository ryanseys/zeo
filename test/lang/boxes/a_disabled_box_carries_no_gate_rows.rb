#@ ruby: -W:no-experimental
# `Ruby::Box`'s gate-only rows exist only under `RUBY_BOX=1`, as CRuby's do.
#
# zeo's rows live in a static table the `ruby_class!` DSL builds at compile
# time, so they are registered either way -- the GATE has to hide them. That
# is what `builtins::gate` already does for the require-gated `IO` rows
# (`io/console`, `io/nonblock`), so `Ruby::Box` joins the same projection
# filter rather than needing a class-side `undef` mechanism of its own. The
# gate is keyed on the environment instead of on `$LOADED_FEATURES`, which is
# the only thing that differs.
#
# Nothing could reach a WRONG answer through this before -- all six refused
# with the gate's own message -- but reflection listed six names CRuby does
# not, which is what a library checking `respond_to?` sees.
#
# The harness runs a box source under `RUBY_BOX=1` when the SOURCE TEXT
# allocates a box -- a plain substring test, so even naming that call in a
# comment flips the very gate this file is testing. The enabled surface is
# `tests/ruby_box_surface.rb`'s.

p (Ruby::Box.singleton_methods(false) - %i[current enabled?]).sort
p (Ruby::Box.instance_methods(false) -
   %i[eval inspect load load_path require require_relative]).sort
p Ruby::Box.enabled?
p Ruby::Box.current
begin
  Ruby::Box.main
rescue Exception => e
  p e.class
end
p Ruby::Box.respond_to?(:root)
p Ruby::Box.method_defined?(:root?)
__END__
[]
[]
false
nil
NoMethodError
false
false
