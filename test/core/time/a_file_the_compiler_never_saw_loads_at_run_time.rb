# A file the compiler never saw is compiled and run where it is FOUND.
#
# Whole-program AOT resolves an ordinary `require` at compile time and
# splices it, and a COMPUTED one under a compile-time `-I` root reaches a
# unit the eager sweep compiled in. What neither covers is a target under
# no compile-time root at all: `$LOAD_PATH.unshift(dir); require "x"`, or
# an absolute path. Those used to be a flat `LoadError`.
#
# They reach the same compiler an `eval` does now
# (`features::UnitCompiler`, installed by the same `zeo_eval_install`), so
# a program that can reach one carries exactly the machinery a program
# that can `eval` carries -- and one that cannot still links neither.
#
# The file is a top-level scope like any other: its `def`s land on
# `Object`, its classes mint, `__FILE__` is its own path, a backtrace row
# inside it says where the code really is, and its own `require`s come
# straight back to the load path -- including `require_relative`, which
# resolves against the CALLING file the frame carries, since a snippet has
# no compile-time directory to resolve against.
root = File.join(__dir__, "a_file_the_compiler_never_saw_loads_at_run_time")
$LOAD_PATH.unshift(root)

# Once only, and the answer says which time it was.
p require("top")
p require("top")
p [$top_runs, $inner_runs]
p RuntimeTop.hi
p LOADED_TOP_FILE.end_with?("top.rb")
p $LOADED_FEATURES.count { |f| f.end_with?("top.rb") }

# A require CYCLE terminates, exactly as CRuby's loading table makes it.
p require("cycle_a")
p [CYCLE_A, CYCLE_B]

# A file that RAISED is not loaded, so a later require retries it.
2.times do
  begin
    require "boom"
  rescue ArgumentError => e
    p [:raised, e.message]
  end
end

# A target that resolves to nothing is still CRuby's LoadError.
begin
  require "no_such_feature_at_all"
rescue LoadError => e
  p [:missing, e.message]
end

# `load` names an exact file and RE-EXECUTES it.
p load(File.join(root, "deep", "inner.rb"))
p $inner_runs

# A backtrace names the file, the line and the scope the code was written
# in -- through two levels of run-time load.
begin
  Raiser.new.boom
rescue RuntimeError => e
  puts e.message
  puts e.backtrace.first(3).map { |l| l.sub(__dir__ + "/", "") }
end
__END__
true
false
[1, 1]
"top 7"
true
1
true
[1, 2]
[:raised, "boom from the file"]
[:raised, "boom from the file"]
[:missing, "cannot load such file -- no_such_feature_at_all"]
true
2
from inner
a_file_the_compiler_never_saw_loads_at_run_time/deep/inner.rb:6:in 'RuntimeInner.boom'
a_file_the_compiler_never_saw_loads_at_run_time/top.rb:10:in 'Raiser#boom'
a_file_the_compiler_never_saw_loads_at_run_time.rb:58:in '<main>'
