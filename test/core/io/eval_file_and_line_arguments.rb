# `eval`'s 3rd and 4th arguments name the snippet, and a snippet is a real
# scope: it gets a frame of its own, under the label of the BINDING it runs
# in -- not the caller's -- and each statement stamps its own line, counted
# from the `lineno` argument. Below it sits the frame of the entry the source
# came through (`Kernel#eval`, `Binding#eval`, ...).
b = binding
p eval("__FILE__", b, "fake.rb", 100)
p eval("__LINE__", b, "fake.rb", 100)
begin
  eval("raise 'x'", b, "fake.rb", 10)
rescue => e
  p e.backtrace.first
end

# Every entry, and the `(eval at FILE:LINE)` name a snippet with no `file`
# argument takes.
begin; b.eval("raise 'a'", "f.rb", 1); rescue => e; p e.backtrace; end
begin; Object.new.instance_eval("raise 'c'"); rescue => e; p e.backtrace; end
begin; String.class_eval("raise 'd'"); rescue => e; p e.backtrace; end

# The line advances with the snippet's own newlines.
begin
  eval("\n\nraise 'w'", b, "multi.rb", 5)
rescue => e
  p e.backtrace.first
end

# An eval under a binding captured inside a METHOD runs in that method's name.
def outer
  bb = binding
  eval("raise 'm'", bb, "f1.rb", 1)
rescue => e
  p e.backtrace.first(2)
end
outer

# A method DEFINED by an eval reports the snippet it was written in, at the
# `def`'s own line -- not the eval's first line.
begin
  eval("def m_in_eval\n  raise 'q'\nend\nm_in_eval", b, "deffile.rb", 3)
rescue => e
  p e.backtrace.first(3)
end
__END__
"fake.rb"
100
"fake.rb:10:in '<main>'"
["f.rb:1:in '<main>'", "core/io/eval_file_and_line_arguments.rb:17:in 'Binding#eval'", "core/io/eval_file_and_line_arguments.rb:17:in '<main>'"]
["(eval at core/io/eval_file_and_line_arguments.rb:18):1:in '<main>'", "core/io/eval_file_and_line_arguments.rb:18:in 'BasicObject#instance_eval'", "core/io/eval_file_and_line_arguments.rb:18:in '<main>'"]
["(eval at core/io/eval_file_and_line_arguments.rb:19):1:in '<main>'", "core/io/eval_file_and_line_arguments.rb:19:in 'Module#class_eval'", "core/io/eval_file_and_line_arguments.rb:19:in '<main>'"]
"multi.rb:7:in '<main>'"
["f1.rb:1:in 'Object#outer'", "core/io/eval_file_and_line_arguments.rb:31:in 'Kernel#eval'"]
["deffile.rb:4:in 'Object#m_in_eval'", "deffile.rb:6:in '<main>'", "core/io/eval_file_and_line_arguments.rb:40:in 'Kernel#eval'"]
