# `include` from inside a box reaches the class's ancestry rather than its
# own tables, and stays the box's.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("module BoxMix; def mixed = 'm'; end; class Array; include BoxMix; end")
p [].respond_to?(:mixed)
p b.eval("[].mixed")
__END__
false
"m"
