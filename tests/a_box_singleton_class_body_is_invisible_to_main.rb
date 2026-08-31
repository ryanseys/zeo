b = Ruby::Box.new
b.eval("class Hash; class << self; def qq = 1; end; end")
p Hash.respond_to?(:qq)
p b.eval("Hash.qq")
