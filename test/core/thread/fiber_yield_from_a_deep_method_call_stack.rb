# The zeo-fiber shim's raison d'etre: suspension from inside a chain
# of ordinary compiled method frames that never saw a yielder.

class Chain
  def a; b; end
  def b; c; end
  def c
    Fiber.yield :from_deep
    :done
  end
end
ch = Chain.new
f = Fiber.new { ch.a }
puts f.resume
puts f.resume
__END__
from_deep
done
