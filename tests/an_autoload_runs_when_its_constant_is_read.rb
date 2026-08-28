# Reading a constant is what RUNS its `autoload`.
#
# zeo recorded the target and never ran it, except for the one internal case
# of a concealed builtin class. So every `autoload` a gem declared was a
# `LoadError` waiting to happen: the read went straight to `const_missing`,
# which found the record and raised the error the load was never given a
# chance to avoid. rubygems declares every one of its classes this way.

DIR = File.expand_path("fixtures/autoload_target", __dir__)

# --- the plain case, by feature name --------------------------------------
$LOAD_PATH.unshift(DIR)

module ByFeature
  autoload :Widget, "autoload_widget"
end

puts "before\t#{ByFeature.const_defined?(:Widget)}"
puts "autoload?\t#{ByFeature.autoload?(:Widget)}"
puts "read\t#{ByFeature::Widget}"
puts "value\t#{ByFeature::Widget.new.name}"
# Spent once it has run.
puts "after\t#{ByFeature.autoload?(:Widget).inspect}"

# --- by ABSOLUTE path, which is how rubygems writes it --------------------
module ByPath
  autoload :Gadget, File.expand_path("autoload_gadget", DIR)
end

puts "path read\t#{ByPath::Gadget}"
puts "path value\t#{ByPath::Gadget.new.name}"

# --- const_get reaches it too, including through a scoped name ------------
module Scoped
  autoload :Thing, "autoload_thing"
end

puts "const_get\t#{Object.const_get('Scoped::Thing')}"

# --- a target that does not exist still raises LoadError ------------------
module Missing
  autoload :Nothing, "no_such_file_anywhere"
end

begin
  Missing::Nothing
rescue LoadError => e
  puts "missing\tLoadError"
end

# --- reading twice does not load twice ------------------------------------
puts "count\t#{$autoload_widget_loads}"
ByFeature::Widget
puts "count again\t#{$autoload_widget_loads}"
