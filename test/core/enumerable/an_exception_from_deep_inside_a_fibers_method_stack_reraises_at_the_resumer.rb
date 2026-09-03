class Deep
  def go; boom; end
  def boom
    raise "deep boom"
  end
end
d = Deep.new
f = Fiber.new { d.go }
begin
  f.resume
rescue RuntimeError => e
  puts "resumer caught: #{e.send(:message)}"
end
__END__
resumer caught: deep boom
