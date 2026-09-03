#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
b = Ruby::Box.new
p b.eval("__FILE__")
begin
  b.eval("raise 'x'")
rescue RuntimeError => e
  p e.backtrace.first(2)
end
__END__
"eval"
["eval:1:in '<compiled>'", "gaps/a_box_literal_eval_is_spliced.rb:6:in 'Ruby::Box#eval'"]
