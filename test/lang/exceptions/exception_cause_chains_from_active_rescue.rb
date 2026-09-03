begin
  begin
    raise "inner"
  rescue
    raise "outer"
  end
rescue => e
  puts e.message
  puts e.cause.message
  puts e.cause.class
end
begin
  raise "solo"
rescue => e
  p e.cause
end
begin
  begin
    Integer("x")
  rescue
    raise ArgumentError, "wrapped"
  end
rescue => e
  puts e.cause.class
end
__END__
outer
inner
RuntimeError
nil
ArgumentError
