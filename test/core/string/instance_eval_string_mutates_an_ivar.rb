class Counter
  def initialize
    @n = 0
  end
end
c = Counter.new
c.instance_eval("@n = @n + 3")
puts c.instance_eval("@n")
__END__
3
