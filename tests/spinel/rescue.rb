# Test rescue/raise/ensure

def safe_div(a, b)
  if b == 0
    raise "division by zero"
  end
  a / b
end

# Basic rescue
begin
  result = safe_div(10, 2)
  puts result  # 5
rescue
  puts "error"
end

# Rescue with exception
begin
  result = safe_div(10, 0)
  puts result
rescue => e
  puts e  # division by zero
end

# Ensure
def with_ensure
  puts "start"
  raise "oops"
  puts "unreachable"
rescue => e
  puts e  # oops
ensure
  puts "cleanup"
end
with_ensure

# Retry pattern
attempts = 0
begin
  attempts += 1
  raise "fail" if attempts < 3
  puts attempts  # 3
rescue
  retry if attempts < 3
end

puts "done"
