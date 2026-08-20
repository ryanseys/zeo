# A `break`/`return` out of a block arms a signal and unwinds through the
# landing chain, which pops `$!` itself -- the jump must not pop it too.
def helper
  [1].each do
    begin
      raise "inner"
    rescue
      return 9
    end
  end
end

begin
  raise "outer"
rescue
  r = [1].each do
    begin
      raise "nested"
    rescue
      break 7
    end
  end
  p r
  p $!.message
  p helper
  p $!.message
end
