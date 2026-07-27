# A poly receiver reaching String#match? / #!~ -- a block param over an array
# whose element type never resolved, an element read out of a widened array.
# emit_expr left the sp_RbVal in sp_re_match_p's const char * slot, so the
# generated C did not compile.
RE = /\A-(\w|-\w[\w-]*)\z/

class Flag
  attr_reader :switches
  def check
    out = []
    switches.each do |s|
      out.push(s.match?(RE))
    end
    out
  end
end

f = Flag.new
f.instance_variable_set(:@switches, ["-a", "--long", "x"])
p f.check

mixed = ["-a", 1, nil]
p(mixed[0].match?(RE))
p(mixed[0] !~ RE)
# nil answers true for !~ (NilClass#=~ is nil) ...
p(mixed[2] !~ RE)
# ... but has no match? at all
begin
  p mixed[2].match?(RE)
rescue NoMethodError => e
  puts "NoMethodError"
end
# and a non-string, non-nil value has neither
begin
  p mixed[1] !~ RE
rescue NoMethodError => e
  puts "NoMethodError"
end
