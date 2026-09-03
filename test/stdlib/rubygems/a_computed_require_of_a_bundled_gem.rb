# `x = ["json", ""].first; require x` loads json, exactly as the same require
# spelled literally does.
#
# This was a recorded divergence, and the reasoning it carried was half right.
# It said the automatic tiers could not reach a bundled gem: the eager unit
# sweep compiles in files on a COMPILE-TIME load path and a bundled gem is not
# on one, while the run-time tier searches `$LOAD_PATH` on disk and a compiled
# binary carries no path to a gem that was never installed on the machine.
# Both true, and both beside the point for json.
#
# json is a NATIVE feature: its Rust half is linked into the binary and
# registered before line 1, which is why a literal `require "json"` folds to
# nothing but a loaded-feature marker. Nothing has to be found, embedded or
# compiled -- the answer was already in the process, and the run-time require
# simply did not know to look. It does now
# (`features::load_builtin_feature`), and the compiler registers every gated
# builtin CONCEALED for a program with a computed require, so `JSON` starts
# absent and this line reveals it.
#
# `--embed-sources` is still the answer for a gem that really is only Ruby
# source; `e2e/gems_require.rs` pins that tier. The neighbours still say how
# the rest is covered: a computed require of a file under a compile-time `-I`
# root works through the unit sweep, and one on the run-time `$LOAD_PATH`
# works through the disk tier
# (`a_file_the_compiler_never_saw_loads_at_run_time.rb`).
name = ["json", ""].first
require name
p JSON.generate([1, 2])

# Registering every gated builtin for this program does NOT reveal them: a
# feature nothing required keeps its constant absent, which is ruby's rule.
begin
  Object.const_get("StringScanner")
  puts "revealed early"
rescue NameError
  puts "still absent"
end
__END__
"[1,2]"
still absent
