# A `begin` whose rescue clause ends in a `puts` answers nil, so the caller's ternary takes the other branch.
# (spinel issue #2900)
class Resp
  def code = 200
end
class Log
  def self.error(msg) = puts(msg)
end
def fetch(fail)
  begin
    raise "boom" if fail
    Resp.new
  rescue => e
    Log.error("failed: #{e.message}")
  end
end
r = fetch(false)
puts r ? r.code : -1
r2 = fetch(true)
puts r2 ? r2.code : -1
__END__
200
failed: boom
-1
