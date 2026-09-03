# MILESTONE: bundler's whole command table, which Thor builds through
# `method_added`.
#
# `Bundler::Thor::Base::ClassMethods#method_added` turns each `def` in
# `Bundler::CLI` into a registered command. zeo refused every one of those
# announcements: `analyze::def_hooks::fires` cannot order a hook against a
# definition in ANOTHER file, and thor and the CLI are different files. So
# `Bundler::CLI` kept 3 of its ~30 commands -- the three `register` adds
# directly -- and `zeo bundle install` answered `Could not find command
# "install"`.
#
# A require GRAPH orders them. `bundler/cli.rb` opens with `require_relative
# "vendored_thor"` and declares `class CLI < Thor` below it, so thor has run
# whole by the time any of those definitions execute -- a fact, not a guess
# about unit order.
#
# This lived as a gap file, where it proved nothing: compiling bundler needs
# more than the golden tier's 512 MiB child bound, so the run was KILLED and
# the gap "passed" on a divergence that was really a dead child. The milestone
# tier is where a whole-graph compile belongs.
#
# Shapes, never versions -- see `tests/milestones.rs`.

require "bundler"
require "bundler/cli"

commands = Bundler::CLI.commands
p commands.keys.size >= 30
%w[install update exec check config list show info add remove lock].each do |name|
  p [name, commands.key?(name)]
end
p Bundler::CLI.all_commands.key?("install")

# Thor runs a command only when the instance reports it PUBLIC, which asks
# `private_methods` -- and `exec` is also a private `Kernel` method, so a name
# left unclaimed by the class that defines it reads back as private.
p Bundler::CLI.private_instance_methods.include?(:exec)
p Bundler::CLI.public_instance_methods.include?(:exec)
__END__
true
["install", true]
["update", true]
["exec", true]
["check", true]
["config", true]
["list", true]
["show", true]
["info", true]
["add", true]
["remove", true]
["lock", true]
true
false
true
