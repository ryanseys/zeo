#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
# A literal `box.eval` is spliced at compile time, and still reports the file
# `eval` and ruby's `<compiled>` and `Ruby::Box#eval` frames.
b = Ruby::Box.new
p b.eval("__FILE__")
begin
  b.eval("raise 'x'")
rescue RuntimeError => e
  p e.backtrace.first(2)
end
__END__
"eval"
["eval:1:in '<compiled>'", "lang/boxes/a_box_literal_eval_is_spliced.rb:8:in 'Ruby::Box#eval'"]
