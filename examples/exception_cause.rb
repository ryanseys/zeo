# Exception#cause: the exception being handled when a new one is raised
# is threaded in automatically as the new exception's cause.
begin
  begin
    raise "root failure"
  rescue
    raise TypeError, "while handling"
  end
rescue => e
  puts e.class
  puts e.message
  puts e.cause.class
  puts e.cause.message
end

# A top-level raise (no active rescue) has no cause.
begin
  raise "lonely"
rescue => e
  p e.cause
end

# A builtin-raised error inside a rescue also chains.
begin
  begin
    Integer("not a number")
  rescue
    raise "rewrapped"
  end
rescue => e
  puts e.cause.class
  puts e.cause.message
end

# Three-deep chain.
begin
  begin
    begin
      raise "level 1"
    rescue
      raise "level 2"
    end
  rescue
    raise "level 3"
  end
rescue => e
  puts e.message
  puts e.cause.message
  puts e.cause.cause.message
  p e.cause.cause.cause
end
