b = Ruby::Box.new
b.eval("class Array; attr_accessor :tag; end")
p [].respond_to?(:tag)
p b.eval("a = []; a.tag = 5; a.tag")
