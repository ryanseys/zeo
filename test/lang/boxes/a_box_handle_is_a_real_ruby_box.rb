#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
# `Ruby::Box.new` answers a value that IS a `Ruby::Box`.
#
# In CRuby `Ruby::Box` is a `Module` subclass, so zeo's
# `RubyValue::Class(surrogate)` representation is structurally RIGHT --
# `b::W` resolves for the same reason. What was wrong was `.class`: it
# answered `Module`, so `b.is_a?(Ruby::Box)` was false and `b.eval`
# silently resolved `Kernel#eval` instead of `Ruby::Box#eval`. The
# class-of walk has an arm for the box table now, beside the two it
# already had (a `class X < Module` instance, a refinement holder).
#
# The other half of what this file used to record was the ENV GATE:
# `Ruby::Box.new` answered a box without `RUBY_BOX=1`, where CRuby raises.
# The gate is a RUN-TIME question, so asking it during the compile that
# allocated the box would bake the build machine's environment into the
# program; the handle comes from `zeo_rt_box_handle` instead, which asks
# it where the program runs.

b = Ruby::Box.new
p b.class.to_s
p b.is_a?(Ruby::Box)
p b.main?
__END__
"Ruby::Box"
true
false
