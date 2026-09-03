#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
b.eval("class Array; class << self; alias_method :zz2, :new; end; end")
p Array.respond_to?(:zz2)
__END__
false
