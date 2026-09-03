# The RUN-TIME compiler on shapes real gems use. A `require` whose target only
# the running program knows -- a computed feature name, or one reached through
# a `$LOAD_PATH` the program just changed -- is compiled at run time by a
# second compiler, and it used to refuse or mis-lower two ordinary things.
#
# AN ALIAS THROUGH `extend`. A method has two installed shapes: one taking an
# `&RObj`, one taking a plain value. Only the second can run with a CLASS as
# `self`. `runtime_alias_method` copied the first and not the second, so an
# aliased method reached through `extend` ran against a surrogate object and a
# receiverless call inside it looked for an INSTANCE method. Forwardable is
# exactly this -- `alias def_delegators def_instance_delegators` -- so
# `extend Forwardable; def_delegators :@t, :year` raised
# `undefined method 'def_instance_delegator'`.
#
# A GUARDED TOP-LEVEL DEFINITION. `analyze::static_guards` rewrites one into a
# synthesized `class Object` wrapper around a synthesized `if`, and neither
# carried a span. A class body compiled at run time is re-evaluated from its
# own SOURCE TEXT, sliced between its first and last statement -- and the slice
# is taken only when the wrapper's span CONTAINS them. Span-less, it refused
# the whole file: "a span-less `class` (`Object`) inside an `eval`". `time.rb`
# has this shape, so it took `require "bundler/setup"` down with it.

DIR = File.expand_path("../../fixtures/runtime_compiled", __dir__)
$LOAD_PATH.unshift(DIR)

# Computed, so the loader cannot resolve it and the run time must.
%w[aliased_extend guarded_toplevel].each { |f| require f }

p UsesAliasedExtend.collected
p GUARDED_WHICH
p guarded_choice
p GuardedHolder.new.which
p UsesAliasedExtend.singleton_class.ancestors.include?(AliasedExtend)
p UsesAliasedExtend.respond_to?(:collect)
p UsesAliasedExtend.method(:collect).owner
__END__
["a@UsesAliasedExtend", "b@UsesAliasedExtend", "c@UsesAliasedExtend"]
:else_branch
:else_branch
:else_branch
true
true
AliasedExtend
