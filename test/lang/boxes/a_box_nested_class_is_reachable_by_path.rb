#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("module Outer; class Inner; def self.who = 'in'; end; end")
p b::Outer::Inner.who
p(begin; Outer; rescue NameError; :namee; end)
__END__
"in"
:namee
