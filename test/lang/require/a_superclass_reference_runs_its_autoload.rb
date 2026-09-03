# `class Sub < Base` READS the constant `Base`, and a read is what runs an
# `autoload` target. zeo resolves the superclass edge at compile time, so the
# read had no run-time half at all: `sub.rb` linked to a `Base` whose body had
# never run, and `Base#limit` raised `uninitialized constant Base::LIMIT` for a
# constant the body assigns.
#
# bundler is the case. `plugin/dsl.rb` requires nothing: `class DSL <
# Bundler::Dsl` is the only thing that loads `bundler/dsl.rb`, and that file's
# class body is where `VALID_KEYS` comes from -- so `bundle install` died in
# `Dsl#git_source` before it read the Gemfile.

autoload :Base, "#{__dir__}/a_superclass_reference_runs_its_autoload/base"
require_relative "a_superclass_reference_runs_its_autoload/sub"
p Sub.new.limit
p Sub.superclass
__END__
base.rb ran
7
Base
