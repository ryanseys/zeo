class MyClass
  attr_reader :switches
  def initialize(opts)
    @switches = []
    @switches += opts
    @switches += []
    @more = [1]
    @more += [2, 3]
  end
  attr_reader :more
end
obj = MyClass.new(["--name", "-n"])
p obj.switches
p obj.more
