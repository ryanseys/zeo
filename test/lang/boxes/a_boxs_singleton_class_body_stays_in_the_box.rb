# The same isolation through a `class << self` body rather than a
# `def self.x`, which reaches the class-method channel by another route.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class Hash; class << self; def qq = 1; end; end")
p Hash.respond_to?(:qq)
p b.eval("Hash.qq")
__END__
false
1
