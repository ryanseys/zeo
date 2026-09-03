# The instance channel, which had the box axis already -- kept here so the
# pair reads together and a regression on either half is one file to look
# at.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class String; def shout = 'S'; end")
p "a".respond_to?(:shout)
p(begin; "a".shout; rescue NoMethodError; :nome; end)
p b.eval("'a'.shout")
__END__
false
:nome
"S"
