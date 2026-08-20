# `raise ..., cause: c` names the cause outright, so the automatic `$!`
# chaining is skipped and the named one stands -- `cause: nil` suppresses
# chaining entirely, and a non-exception cause is a TypeError.
class Wrapped < StandardError; end
begin
  begin
    raise "inner"
  rescue => e
    raise Wrapped, "outer", cause: e
  end
rescue Wrapped => w
  p w.message
  p w.cause.message
end

begin
  raise "a", cause: nil
rescue => e
  p e.cause
end

begin
  begin
    raise "x"
  rescue
    raise ArgumentError.new("y"), cause: $!
  end
rescue ArgumentError => e
  p [e.message, e.cause.message]
end

begin
  raise "z", cause: 5
rescue TypeError => e
  p e.message
end

err = RuntimeError.new("self")
begin
  raise err, cause: err
rescue => e
  p e.cause
end
