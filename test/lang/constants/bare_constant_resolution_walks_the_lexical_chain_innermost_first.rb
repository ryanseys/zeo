X = "top"
module Outer
  X = "outer"
  module Inner
    X = "inner"
    def self.probe
      X
    end
  end
  def self.probe
    X
  end
end
puts Outer::Inner.probe
puts Outer.probe
puts X
__END__
inner
outer
top
