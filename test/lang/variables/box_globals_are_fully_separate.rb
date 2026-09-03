# Gvar isolation is BIDIRECTIONAL (separate per-box tables, no fallback):
# a box reads nil for main's `$g`, and a box's write never reaches main.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

$g = "main value"
box = Ruby::Box.new
p box.eval("$g")
box.eval("$g = 'box value'")
p $g
p box.eval("$g")
__END__
nil
"main value"
"box value"
