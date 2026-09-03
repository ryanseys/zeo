#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("module BoxMix; def mixed = 'm'; end; class Array; include BoxMix; end")
p [].respond_to?(:mixed)
p b.eval("[].mixed")
__END__
false
"m"
