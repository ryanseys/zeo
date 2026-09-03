#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; @@cv = 3; def self.cv = @@cv; end")
p(begin; Array.cv; rescue NoMethodError; :nome; end)
p b.eval("Array.cv")
__END__
:nome
3
