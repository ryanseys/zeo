# A NON-literal `box.eval` source (a variable, a computed string) routes
# through the runtime `eval` in the box's dimension -- the same fall-through
# `Kernel#eval` uses for a dynamic source, but carrying `box_id`. A dynamic
# eval reads/calls the box's own (`require`-defined) constants and classes,
# its globals stay isolated from main, and a non-String source raises a
# catchable `TypeError` at runtime rather than a compile error.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

require_relative "box_eval_dynamic_source_routes_through_the_vm/main"
__END__
3
"widget!"
"a widget"
nil
"main value"
"box value"
rescued: no implicit conversion of Integer into String
