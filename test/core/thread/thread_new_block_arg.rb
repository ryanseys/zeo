# `Thread.new(&proc)` -- drb starts its main loop that way. The block argument
# converts exactly as it does at every other call site.
def worker(n) = (puts "worker #{n}")

Thread.new(3, &method(:worker)).join

blk = proc { |a, b| puts "proc #{a}#{b}" }
Thread.new(1, 2, &blk).join

q = Queue.new
Thread.new(&-> { q << :from_lambda }).join
p q.pop

# A Symbol converts through `Symbol#to_proc`, so the thread argument becomes
# the receiver.
Thread.new("shout", &:upcase).join
p Thread.new("shout", &:upcase).value

# `&nil` is "no block", which `Thread.new` rejects the same way a missing
# literal one is rejected.
begin
  Thread.new(&nil)
rescue ThreadError => e
  puts e.message
end
begin
  Thread.new
rescue ThreadError => e
  puts e.message
end
__END__
worker 3
proc 12
:from_lambda
"SHOUT"
must be called with a block
must be called with a block
