# Matching a UTF-8 regexp against an EUC-JP string raises
# Encoding::CompatibilityError. zeo's =~ answers nil, #match? answers
# TRUE, and #match builds a MatchData over the reinterpreted bytes --
# silent data corruption, not just a missing error. A String pattern to
# #gsub with an incompatible encoding substitutes on a wrong match the
# same way. The #index/#sub/#split/#scan regexp paths already raise
# correctly, so the guard exists and these entry points skip it. (Found
# by the 2026-08-24 probe sweep.)
e = "日".encode("EUC-JP")
def show
  p yield
rescue Exception => ex
  puts "#{ex.class}: #{ex.message}"
end
show { e =~ /x/ }
show { e.match?(/x/) }
show { e.match(/x/) }
show { "日".encode("Shift_JIS").gsub("日".encode("EUC-JP"), "x") }
