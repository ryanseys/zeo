obj = Object.new
obj.instance_variable_set(:@v, 3)
read = "@v * 2"
p obj.instance_eval(read)
defn = "def dbl; @v * 4; end"
obj.instance_eval(defn)
p obj.dbl
__END__
6
12
