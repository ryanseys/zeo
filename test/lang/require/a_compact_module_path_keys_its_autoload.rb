# `module A::B` -- the COMPACT form -- contributes both segments to the
# lexical path a constant inside it hangs off. The autoload collector read
# prism's `name` on that node, which is the LAST segment alone, so it keyed
# `autoload :Widget, "..."` under `Nest::Widget` where the read resolves
# `Deep::Nest::Widget`. Nothing ever matched, and the read that should have
# run the target did nothing -- silently, because a compiled-in unit's
# classes are in the dispatch tables from startup. `Deep::Nest::Widget`
# resolved and `.kind` was callable; only the body's constant was missing.
#
# rspec-core writes `module RSpec::Core::Formatters` and declares nine
# formatter autoloads inside it, so none of them ever loaded: reading
# `ProgressFormatter` gave a class whose body had never run, and nothing had
# registered it as a formatter.

module Deep
  module Nest
  end
end

module Deep::Nest
  autoload :Widget, "#{__dir__}/a_compact_module_path_keys_its_autoload/target"
end

puts "before read"
p Deep::Nest.autoload?(:Widget) != nil
p Deep::Nest::Widget.kind
p Deep::Nest.autoload?(:Widget) != nil
__END__
before read
true
target.rb ran
:widget
false
