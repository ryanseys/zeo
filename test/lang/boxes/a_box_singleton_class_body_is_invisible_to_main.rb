#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Hash; class << self; def qq = 1; end; end")
p Hash.respond_to?(:qq)
p b.eval("Hash.qq")
__END__
false
1
