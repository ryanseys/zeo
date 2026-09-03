# A `yield` reached with no block is a RESCUABLE LocalJumpError, not a
# process abort -- including for a singleton method, whose block is
# threaded through the call site.

obj = Object.new
def obj.needs_block
  yield
end
begin
  obj.needs_block
rescue LocalJumpError => e
  puts "caught #{e.class}: #{e.message}"
end
__END__
caught LocalJumpError: no block given (yield)
