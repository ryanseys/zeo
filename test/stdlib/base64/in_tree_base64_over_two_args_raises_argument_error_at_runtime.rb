# The in-tree Base64 ext checks arity at runtime (a rescuable
# ArgumentError) via `arity!`.

require "base64"
begin
  Base64.encode64("a", "b")
rescue ArgumentError
  puts "argerr"
end
__END__
argerr
