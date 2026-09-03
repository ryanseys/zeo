# `rescue M => e` matches via `===` (CRuby's rule) -- a matcher module that
# OVERRIDES `self.===` decides for itself. rspec-support's
# AllExceptionsExceptOnesWeMustNotRescue catches everything except
# NoMemoryError/SignalException this way; an ancestry-only match let real
# expectation failures escape Example#run.
module CatchesRuntime
  def self.===(exception)
    RuntimeError === exception
  end
end

begin
  raise "boom"
rescue CatchesRuntime => e
  puts "caught: #{e.message}"
end

begin
  raise TypeError, "nope"
rescue CatchesRuntime => e
  puts "wrong: #{e.message}"
rescue TypeError => e
  puts "type: #{e.message}"
end
__END__
caught: boom
type: nope
