f = Fiber.new do
  3.times do |i|
    Fiber.yield i
  end
  :end
end
puts f.resume
puts f.resume
puts f.resume
puts f.resume
__END__
0
1
2
end
