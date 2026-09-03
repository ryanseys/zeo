src = "1 +"
b = TOPLEVEL_BINDING
Zeo::Eval.prepare(src, b, "bad.rb", 1)
status = nil
50.times do
  status = Zeo::Eval.prepare(src, b, "bad.rb", 1)
  break if status == :failed
  sleep 0.02
end
p status
begin
  eval(src, b, "bad.rb", 1)
rescue SyntaxError
  puts "SyntaxError"
end
__END__
:failed
SyntaxError
