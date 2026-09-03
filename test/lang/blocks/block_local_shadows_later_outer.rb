# Ruby's block-local rule is TEXTUAL: a name assigned inside a block is
# block-local unless the enclosing scope assigned it EARLIER in the source.
# zeo's capture analysis (codegen::captures::collect_escaping_captures) is
# order-blind -- it unions every name an escaping block touches against every
# name the scope assigns ANYWHERE, so a block-local that happens to share a
# name with a LATER outer local becomes a shared Captured cell. The block's
# write then leaks into the outer variable. The same misclassification makes
# `Ractor.new { e = 1 }` refuse isolation when main rescue-binds an `e`
# further down. Fix: the capture set must only admit names whose outer
# assignment PRECEDES the block textually.
b = proc { x = 1; x }
x = 5
b.call
p x

r = proc { note = "inner"; note.upcase }
p r.call
note = "outer"
p note
__END__
5
"INNER"
"outer"
