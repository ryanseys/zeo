t1 = Thread.new do
  begin
    raise "in thread"
  rescue RuntimeError => e
    "rescued"
  ensure
    x = 1
  end
end
puts t1.value
t2 = Thread.new do
  attempts = 0
  begin
    attempts += 1
    raise "flaky" if attempts < 3
    attempts
  rescue RuntimeError
    retry
  end
end
puts t2.value
__END__
rescued
3
