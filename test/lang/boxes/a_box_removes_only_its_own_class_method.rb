#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class String; def self.zz = 1; end")
b.eval("class String; class << self; remove_method :zz; end; end")
p(begin; b.eval("String.zz"); rescue NoMethodError; :nome; end)
p String.respond_to?(:zz)
__END__
:nome
false
