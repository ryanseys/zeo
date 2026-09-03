# A box's patch of a shared class must not follow a value BACK to main:
# the Array built inside the box is an ordinary Array in main, and calling
# the box's method on it raises there.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class Array; def self.zzz = 9; def mine = 'm'; end")
a = b.eval("[1, 2]")
p a
p a.class == Array
p(begin; a.mine; rescue NoMethodError; :nome; end)
__END__
[1, 2]
true
:nome
