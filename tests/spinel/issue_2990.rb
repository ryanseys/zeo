def m
  raise "inner"
rescue
  raise "outer", cause: nil
end
begin
  m
rescue => e
  p e.message
  p e.cause
end

# an explicit non-nil cause is still threaded
def m2
  raise "first"
rescue => a
  begin
    raise "second"
  rescue
    raise "third", cause: a
  end
end
begin
  m2
rescue => e
  p e.cause.message
end

# no cause: keeps the implicit chain
def m3
  raise "lo"
rescue
  raise "hi"
end
begin
  m3
rescue => e
  p e.cause.message
end
