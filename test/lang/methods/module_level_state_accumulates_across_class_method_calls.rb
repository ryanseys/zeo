module Counter
  @n = 0
  def self.bump; @n += 1; end
  def self.n; @n; end
end
Counter.bump
Counter.bump
Counter.bump
p Counter.n
__END__
3
