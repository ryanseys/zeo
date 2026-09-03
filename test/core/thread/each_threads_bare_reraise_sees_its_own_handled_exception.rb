# Each thread's bare `raise` must re-raise its OWN handled exception, never
# a sibling's -- the per-thread handling stack keeps the two isolated.

t1 = Thread.new do
  begin
    raise "from t1"
  rescue RuntimeError => e
    begin
      raise
    rescue RuntimeError => inner
      puts "t1 re-raised: #{inner.send(:message)}"
    end
  end
end
t1.join
t2 = Thread.new do
  begin
    raise "from t2"
  rescue RuntimeError => e
    begin
      raise
    rescue RuntimeError => inner
      puts "t2 re-raised: #{inner.send(:message)}"
    end
  end
end
t2.join
__END__
t1 re-raised: from t1
t2 re-raised: from t2
