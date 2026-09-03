# A Thread mid-rescue resumes a fiber whose bare raise must see an EMPTY
# $!, not the thread's in-flight exception.

t = Thread.new do
  f = Fiber.new do
    begin
      raise
    rescue RuntimeError => e
      "fiber saw: [#{e.send(:message)}]"
    end
  end
  begin
    raise "thread's exception"
  rescue RuntimeError
    f.resume
  end
end
puts t.value
__END__
fiber saw: []
