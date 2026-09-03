class Widget
  def initialize(n)
    @n = n
  end

  def n
    @n
  end
end

x = Widget
puts x.new(5).n
puts x.name
__END__
5
Widget
