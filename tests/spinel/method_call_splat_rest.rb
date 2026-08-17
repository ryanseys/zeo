class Var
  def sum(*a); a.sum; end
end
args001 = [1, 2, 3]
p Var.new.method(:sum).call(*args001)

m002 = Var.new.method(:sum); p m002.call(*args001)   # Ruby: 6

p Var.new.method(:sum).call(1, 2, 3)   # => 6
p Var.new.method(:sum).call(1)         # => 1
p Var.new.method(:sum).call            # => 0
p Var.new.sum(*args001)                # => 6
p Var.new.sum(1, 2, 3)                 # => 6
