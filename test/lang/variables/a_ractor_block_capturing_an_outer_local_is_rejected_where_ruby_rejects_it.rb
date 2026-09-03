# CRuby's `ArgumentError`, raised by `Ractor.new` itself and catchable
# there -- not the compile error zeo used to report. The verdict rides on
# the Proc value (`RProc::with_outer_capture`), so a literal block and a
# `&proc` argument reach the same raise.

x = 5
begin
  Ractor.new { x + 1 }
rescue => e
  puts "raised: #{e.class}: #{e.send(:message)}"
end
puts "still running"
__END__
raised: ArgumentError: can not isolate a Proc because it accesses outer variables (x).
still running
#@ stderr
lang/variables/a_ractor_block_capturing_an_outer_local_is_rejected_where_ruby_rejects_it.rb:8: warning: Ractor API is experimental and may change in future versions of Ruby.
