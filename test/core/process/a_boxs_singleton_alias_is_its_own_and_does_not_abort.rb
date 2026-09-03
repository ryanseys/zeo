# A class-method ALIAS written in a box's `class << self`. This ABORTED the
# process before: the alias named the box's overlay class, which registers
# no entry, and the registrar's `expect` on that lookup is reached across an
# `extern "C"` boundary that cannot unwind.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class Array; class << self; alias_method :zz2, :new; end; end")
p Array.respond_to?(:zz2)
p b.eval("Array.zz2(1, 5)")
__END__
false
[5]
