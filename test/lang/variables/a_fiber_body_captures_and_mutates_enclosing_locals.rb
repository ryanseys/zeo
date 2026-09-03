# The fiber's block goes through the ordinary escaping-Proc capture
# machinery (Arc<Mutex> cells), so shared mutation across
# suspension points works exactly like any other escaping block.

count = 0
c = Fiber.new do
  count += 10
  Fiber.yield
  count += 100
end
c.resume
puts count
c.resume
puts count
__END__
10
110
