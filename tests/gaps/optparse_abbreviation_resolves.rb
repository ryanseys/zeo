# OptionParser resolves a unique long-option PREFIX (`--verb` finds
# `--verbose`) and raises AmbiguousOption when two candidates share it.
# zeo raises InvalidOption for both -- and optparse is vendored Ruby, so
# zeo is mis-running the gem's own `complete` machinery. (Found by the
# 2026-08-24 probe sweep.)
require "optparse"
got = nil
OptionParser.new { |o| o.on("--verbose") { got = :v } }.parse(["--verb"])
p got
begin
  o = OptionParser.new do |op|
    op.on("--foobar") { }
    op.on("--foobaz") { }
  end
  o.parse(["--fooba"])
rescue OptionParser::AmbiguousOption => e
  puts "#{e.class}: #{e.message}"
end
