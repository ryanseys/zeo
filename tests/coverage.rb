# Line coverage over the AOT instrumentation: the lifecycle and its error
# shapes, then the real measurement of coverage/target.rb -- required AFTER
# Coverage.start, so it is reported (this file, loaded before, is not --
# CRuby's own inclusion rule). The line array is compared byte-for-byte
# against the C extension: def lines, class lines, per-call body counts, an
# unentered branch's 0, and the nil non-executable lines.
require "coverage"
p Coverage.supported?(:lines)
p Coverage.state
p Coverage.running?
begin
  Coverage.result
rescue RuntimeError => e
  p [e.class, e.message]
end
begin
  Coverage.supported?("lines")
rescue TypeError => e
  p [e.class, e.message]
end
Coverage.start
p Coverage.state
p Coverage.running?
begin
  Coverage.start
rescue RuntimeError => e
  p [e.class, e.message]
end
require_relative "coverage/target"
c = CovCounter.new(1)
p c.bump
p c.bump
p c.big?
p cov_add(20, 22)
peek = Coverage.peek_result
p peek.size
res = Coverage.result
p Coverage.state
p Coverage.running?
p res.size
res.each do |path, lines|
  puts File.basename(path)
  p lines
end
p peek == res
begin
  Coverage.result
rescue RuntimeError => e
  p [e.class, e.message]
end
