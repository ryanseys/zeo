# A bare `raise` inside a def's rescue clause re-raises the exception being
# handled, class and message intact.

def helper
  begin
    raise StandardError, "inner"
  rescue => e
    raise
  end
end

begin
  helper
rescue => e
  puts "outer: #{e.class}: #{e.message}"
end

# Variant: 2-arg raise as the only statement (no rescue, propagates).
def thrower
  raise ArgumentError, "from thrower"
end

begin
  thrower
rescue ArgumentError => e
  puts "caught: #{e.message}"
end

# Variant: 1-arg raise (string only) in def + outer rescue.
def thrower2
  raise "plain message"
end

begin
  thrower2
rescue => e
  puts "got: #{e.class}: #{e.message}"
end
__END__
outer: StandardError: inner
caught: from thrower
got: RuntimeError: plain message
