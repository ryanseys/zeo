# The same LocalJumpError path an ordinary method has always needed -- it
# used to abort the process with a Rust panic instead of raising.

def m
  yield
end
begin
  m
rescue LocalJumpError => e
  puts "caught #{e.class}"
end
puts m { "with-block" }
__END__
caught LocalJumpError
with-block
