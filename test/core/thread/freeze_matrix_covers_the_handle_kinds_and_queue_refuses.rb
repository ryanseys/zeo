# The frozen flag on the non-collection heap kinds: Proc, Regexp
# (literals frozen at birth, `Regexp.new` not), MatchData, Fiber,
# Enumerator, Thread, Mutex (lockable while frozen) -- with the
# dup-drops/clone-copies flag rule and the flag never aliasing an
# earlier copy. Queue/SizedQueue refuse to freeze with CRuby's
# TypeError. Expected output is verbatim ruby 4.0.6.

pr = proc { 1 }
puts pr.frozen?
d = pr.dup
pr.freeze
puts pr.frozen?
puts d.frozen?
puts pr.dup.frozen?
puts pr.clone.frozen?
x = "b"
puts /abc/.frozen?
puts /a#{x}/.frozen?
rn = Regexp.new("abc")
puts rn.frozen?
rn.freeze
puts rn.frozen?
puts rn.dup.frozen?
puts rn.clone.frozen?
md = "abc".match(/b/)
puts md.frozen?
md.freeze
puts md.frozen?
puts md.dup.frozen?
puts md.clone.frozen?
f = Fiber.new { }
puts f.frozen?
f.freeze
puts f.frozen?
en = [1].each_slice(1)
puts en.frozen?
en.freeze
puts en.frozen?
puts en.dup.frozen?
puts en.clone.frozen?
t = Thread.new { }
t.join
puts t.frozen?
t.freeze
puts t.frozen?
m = Thread::Mutex.new
m.freeze
puts m.frozen?
m.lock
puts m.locked?
m.unlock
q = Thread::Queue.new
begin
  q.freeze
rescue TypeError => e
  puts "#{e.class}: #{e.message[0, 22]}"
end
puts q.frozen?
sq = Thread::SizedQueue.new(1)
begin
  sq.freeze
rescue TypeError => e
  puts e.message.start_with?("cannot freeze #<Thread::SizedQueue:0x")
end
__END__
false
true
false
false
true
true
true
false
true
false
true
false
true
false
true
false
true
false
true
false
true
false
true
true
true
TypeError: cannot freeze #<Thread
false
true
