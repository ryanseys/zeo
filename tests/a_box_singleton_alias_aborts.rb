b = Ruby::Box.new
b.eval("class Array; class << self; alias_method :zz2, :new; end; end")
p Array.respond_to?(:zz2)
