src = "40 + 2"
b = TOPLEVEL_BINDING
status = Zeo::Eval.prepare(src, b, "bench.rb", 1, true)
raise "unexpected #{status}" unless [:compiling, :ready].include?(status)
50.times do
  break if Zeo::Eval.prepare(src, b, "bench.rb", 1, true) == :ready
  sleep 0.02
end
raise "never ready" unless Zeo::Eval.prepare(src, b, "bench.rb", 1, true) == :ready
p eval(src, b, "bench.rb", 1)
__END__
42
