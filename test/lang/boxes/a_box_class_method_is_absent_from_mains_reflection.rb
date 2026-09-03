#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; def self.zzz = 1; end")
p Array.singleton_methods(false).include?(:zzz)
p b.eval("Array.singleton_methods(false).include?(:zzz)")
__END__
false
true
