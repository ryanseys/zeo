# The point of the SOURCE_FILE stack: `require` merges every file's
# statements into one Program, so by codegen time nothing tells them
# apart -- these have to be resolved at LOWERING time, per file. A
# `__FILE__` inside a required file must name THAT file, not the main
# one, and the main file's own `__FILE__` after the require must be
# itself again.
#
# Only basenames are compared: the harness compiles from a per-test
# temp dir, so the absolute paths differ per run.
# helper_line is 2 (the `def helper_line` line); the main file's
# `__LINE__` is on its own 6th line counting the leading newline.
#
# `__dir__` is only checked for absoluteness and for agreeing between
# the two files, NOT compared against `File.expand_path(__FILE__)`:
# `__dir__` is baked at COMPILE time while `expand_path` of a relative
# `__FILE__` resolves against the RUNTIME cwd, and this harness runs
# the binary from a different directory than it compiled in. The two
# agree whenever the program is run from its own directory, which is
# verified against the oracle separately.

require_relative "file_line_and_dir_name_the_file_the_code_was_written_in/main"
__END__
helper.rb
2
main.rb
5
true
true
