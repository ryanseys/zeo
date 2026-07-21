# Thread.pass from the main thread yields one round-robin turn to the runnable
# siblings and then resumes main, rather than draining them to completion. So
# main interleaves with a sibling that also yields, instead of being starved
# until the sibling finishes.
#
# The interleaving order is scheduler-specific (CRuby is preemptive and
# nondeterministic; spinel's Phase 0 scheduler is cooperative and deterministic),
# so this checks spinel's deterministic output. The property that matters: the
# `:main` entries are interleaved with the sibling's, not all bunched before or
# after them.
log = []
t = Thread.new do
  5.times do |i|
    log << i
    Thread.pass
  end
end
3.times do
  log << :main
  Thread.pass
end
t.join
puts log.inspect

# A sibling looping on Thread.pass must not starve main: main reaches here and
# completes its own loop (with the old drain behaviour main would run the
# sibling to completion on its first pass, but it would still complete -- the
# point of the first check above is the *interleaving*).
spinner = Thread.new { 100.times { Thread.pass } }
main_turns = 0
4.times do
  main_turns += 1
  Thread.pass
end
puts "main_turns: #{main_turns}"
spinner.join
puts "done"
