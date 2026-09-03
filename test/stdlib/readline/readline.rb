# readline -- line input off a redirected source (the deterministic leg: no
# terminal, no rendering), the HISTORY object, and the attribute surface.
require "readline"
require "tmpdir"

p Readline.respond_to?(:readline)

path = File.join(Dir.tmpdir, "zeo_readline_input_#{Process.pid}.txt")
File.write(path, "alpha\n\nbeta\n")
Readline.input = File.open(path)
Readline.output = File.open("/dev/null", "w")

p Readline.readline("> ", true)
p Readline.readline("> ", true)
p Readline.readline("> ", true)
p Readline.readline("> ", true)
p Readline::HISTORY.to_a

Readline::HISTORY << "manual"
Readline::HISTORY.push("a", "b")
p [Readline::HISTORY.length, Readline::HISTORY.size, Readline::HISTORY[0], Readline::HISTORY[-1]]
p Readline::HISTORY.include?("manual")
collected = []
Readline::HISTORY.each { |line| collected << line }
p collected
p Readline::HISTORY.delete_at(1)
p Readline::HISTORY.to_a
p Readline::HISTORY.pop
p Readline::HISTORY.shift
Readline::HISTORY[0] = "rewritten"
p Readline::HISTORY.to_a
begin
  Readline::HISTORY[99]
rescue IndexError => e
  p e.class
end
Readline::HISTORY.clear
p [Readline::HISTORY.size, Readline::HISTORY.empty?]

Readline.completion_proc = proc { |s| ["#{s}1", "#{s}2"] }
p Readline.completion_proc.call("x")
p Readline.completion_append_character
Readline.completion_append_character = " "
p Readline.completion_append_character
p Readline.completion_case_fold
Readline.completion_case_fold = true
p Readline.completion_case_fold
p Readline.basic_word_break_characters
Readline.basic_word_break_characters = " \t"
p Readline.basic_word_break_characters
p Readline.basic_quote_characters
p Readline.completer_quote_characters
p Readline.filename_quote_characters
p Readline.special_prefixes
p [Readline.vi_editing_mode?, Readline.emacs_editing_mode?]
p Readline::FILENAME_COMPLETION_PROC
p Readline::USERNAME_COMPLETION_PROC

File.unlink(path)
__END__
true
"alpha"
""
"beta"
nil
["alpha", "beta"]
[5, 5, "alpha", "b"]
true
["alpha", "beta", "manual", "a", "b"]
"beta"
["alpha", "manual", "a", "b"]
"b"
"alpha"
["rewritten", "a"]
IndexError
[0, true]
["x1", "x2"]
nil
" "
nil
true
" \t\n`><=;|&{("
" \t"
"\"'"
"\"'"
""
""
[false, true]
nil
nil
