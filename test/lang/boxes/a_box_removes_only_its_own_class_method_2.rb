# `remove_method` in a box's `class << self` retires the box's own row and
# leaves main's alone. The removal has to find the row first -- it asked
# about the overlay id and was told the method did not exist.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

b = Ruby::Box.new
b.eval("class String; def self.zz = 1; end")
b.eval("class String; class << self; remove_method :zz; end; end")
p(begin; b.eval("String.zz"); rescue NoMethodError; :nome; end)
p String.respond_to?(:zz)
__END__
:nome
false
