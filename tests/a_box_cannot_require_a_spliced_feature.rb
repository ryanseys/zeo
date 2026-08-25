# A box cannot `require` a feature the compiler SPLICED into main: there is
# no file to re-read and no unit of its own to re-run. A decided divergence
# -- the `.divergence` sidecar records why, and `--embed-sources` is the
# opt-in that makes it work. Oracle: the box gets its own copy of json.
require "json"
b = Ruby::Box.new
begin
  b.require "json"
  p b.eval("JSON.generate([1])")
rescue LoadError => e
  p [:load_error, e.message]
end
