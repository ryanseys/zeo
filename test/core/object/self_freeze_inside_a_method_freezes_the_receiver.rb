class Lockable
  def initialize
    @v = 1
  end
  def lock_it
    self.freeze
  end
  def set_v(n)
    @v = n
  end
  def v
    @v
  end
end
l = Lockable.new
l.lock_it
puts l.frozen?
begin
  l.set_v(2)
rescue FrozenError => e
  puts "frozen!"
end
puts l.v
__END__
true
frozen!
1
