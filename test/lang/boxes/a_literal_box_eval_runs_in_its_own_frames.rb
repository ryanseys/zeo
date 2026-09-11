#@ env: RUBY_BOX=1
#@ ruby: -W:no-experimental
# A literal `box.eval("...")` runs inside ruby's two frames: the snippet's
# `<compiled>` scope in file `eval`, under the C method `Ruby::Box#eval` at
# the call's line. A block in the snippet is `block in <compiled>`, and a
# computed source gets the same frames.
b = Ruby::Box.new
p b.eval("__FILE__")
p b.eval("__LINE__")
p b.eval("[__FILE__, __LINE__]\n")
begin
  b.eval("raise 'x'")
rescue RuntimeError => e
  p e.backtrace
end
begin
  b.eval("1\n[2].each { raise 'y' }")
rescue RuntimeError => e
  p e.backtrace
end
src = "raise 'z'"
begin
  b.eval(src)
rescue RuntimeError => e
  p e.backtrace
end
p b.eval("caller(0).first(2)")
p b.eval("40 + 2")
p caller(0).size
__END__
"eval"
1
["eval", 1]
["eval:1:in '<compiled>'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:12:in 'Ruby::Box#eval'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:12:in '<main>'"]
["eval:2:in 'block in <compiled>'", "eval:2:in 'Array#each'", "eval:2:in '<compiled>'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:17:in 'Ruby::Box#eval'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:17:in '<main>'"]
["eval:1:in '<compiled>'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:23:in 'Ruby::Box#eval'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:23:in '<main>'"]
["eval:1:in '<compiled>'", "lang/boxes/a_literal_box_eval_runs_in_its_own_frames.rb:27:in 'Ruby::Box#eval'"]
42
1
