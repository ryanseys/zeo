#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; def self.zzz = 9; end")
p Array.respond_to?(:zzz)
p(begin; Array.zzz; rescue NoMethodError; :nome; end)
p b.eval("Array.zzz")
p b.eval("Array.respond_to?(:zzz)")
p b.eval("Array.new(2, 7)")
__END__
false
:nome
9
true
[7, 7]
