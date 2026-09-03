# Once the home method has unwound -- via exception OR normal return --
# the Proc's `return` finds no live home and raises LocalJumpError rather
# than leaking a Signal::Return.

$escaped = nil
def home_raises; $escaped = proc { return 99 }; raise "boom"; end
begin; home_raises; rescue; end
begin
  $escaped.call; puts "WRONG"
rescue LocalJumpError => e
  puts "exc: #{e.message}"
end

class Deferred
  def arm; @job = proc { return :never }; self; end
  def fire; @job.call; end
end
begin
  Deferred.new.arm.fire; puts "WRONG"
rescue LocalJumpError => e
  puts "norm: #{e.message}"
end

def collect; [proc { return 4 }]; end
arr = collect
begin
  arr[0].call; puts "WRONG"
rescue LocalJumpError => e
  puts "arr: #{e.message}"
end
__END__
exc: unexpected return
norm: unexpected return
arr: unexpected return
