# Exception#backtrace returns an Array of frame strings (method-granularity,
# populated only in --debug builds; release returns []). This guards the
# dispatch shape that's release-stable -- backtrace is an Array, not a
# hardcoded empty literal swallowing the receiver.
begin
  raise "boom"
rescue => e
  bt = e.backtrace
  puts bt.class           # Array
  puts bt.is_a?(Array)    # true
  puts e.message          # boom
end
