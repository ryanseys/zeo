# A visibility violation is RUNTIME behavior in Ruby, not a syntax
# error: it raises NoMethodError when the call actually runs, so it can
# be rescued and an unreachable one stays silent. Message shape and
# both behaviors oracle-verified.

class Box
  def initialize
    @x = 1
  end

  private

  def helper
    2
  end
end

b = Box.new
begin
  b.helper
rescue NoMethodError => e
  puts e.message
end

def never_runs(b)
  b.helper
end
puts "still here"
__END__
private method 'helper' called for an instance of Box
still here
