#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("$boxg = 7")
p $boxg
p b.eval("$boxg")
__END__
nil
7
