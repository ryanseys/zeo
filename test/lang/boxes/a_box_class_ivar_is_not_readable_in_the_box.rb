#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; @civ = 7; def self.civ = @civ; end")
p b.eval("Array.civ")
__END__
7
