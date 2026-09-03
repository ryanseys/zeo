obj = Object.new
class << obj
  def each_pair
    yield 1
    yield 2
    block_given? ? "done" : "noblock"
  end
end
collected = []
r = obj.each_pair { |n| collected << n }
p collected
puts r
__END__
[1, 2]
done
