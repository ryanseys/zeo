# A box minted at RUN TIME, whose id the compiler never saw. Its constant
# has to be written and read under the same box, which is the case that
# broke when the write learned the box before the read did.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

d = [Ruby::Box.new].first
d.eval("RUNTIME_BOX_CONST = 7")
p d.eval("RUNTIME_BOX_CONST")
p defined?(RUNTIME_BOX_CONST)
__END__
7
nil
