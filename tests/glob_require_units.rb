# A glob-driven require loop -- how a test suite (tzinfo's ts_all.rb) or a
# plugin registry loads itself. The target strings exist only at runtime, so
# the demanding file's directory is compiled in as callable units and the
# runtime require resolves what the program builds.
require_relative "glob_require_units/loader"
puts PART_A
puts PART_B
