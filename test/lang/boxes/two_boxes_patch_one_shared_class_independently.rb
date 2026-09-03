# Two boxes patching one shared class do not see each other, and main sees
# neither. One box could be made to work by accident (any box-shaped key
# answers); two cannot.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

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
