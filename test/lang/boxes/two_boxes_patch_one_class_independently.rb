#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
a = Ruby::Box.new
b = Ruby::Box.new
a.eval("class Array; def self.who = 'a'; end")
b.eval("class Array; def self.who = 'b'; end")
p a.eval("Array.who")
p b.eval("Array.who")
p(begin; Array.who; rescue NoMethodError; :nome; end)
__END__
"a"
"b"
:nome
