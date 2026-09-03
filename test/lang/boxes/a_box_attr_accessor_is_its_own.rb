#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; attr_accessor :tag; end")
p [].respond_to?(:tag)
p b.eval("a = []; a.tag = 5; a.tag")
__END__
false
5
