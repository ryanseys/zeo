# Thread.pass from the main thread yields a turn to runnable siblings
# without draining them to completion. On a preemptive parallel scheduler
# (CRuby, and zeo's OS threads) the interleaving ORDER is nondeterministic,
# so this asserts only schedule-independent properties: every entry lands,
# the sibling's own entries stay in its program order, and main is never
# starved out of completing its loop.
log = []
mx = Mutex.new
t = Thread.new do
  5.times do |i|
    mx.synchronize { log << i }
    Thread.pass
  end
end
3.times do
  mx.synchronize { log << :main }
  Thread.pass
end
t.join
puts log.length
puts log.count(:main)
puts log.select { |e| e != :main }.inspect

# A sibling looping on Thread.pass must not starve main: main reaches here
# and completes its own loop regardless of how turns interleave.
spinner = Thread.new { 100.times { Thread.pass } }
main_turns = 0
4.times do
  main_turns += 1
  Thread.pass
end
puts "main_turns: #{main_turns}"
spinner.join
puts "done"
