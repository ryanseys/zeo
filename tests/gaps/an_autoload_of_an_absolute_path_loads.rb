# GAP: `autoload` with an ABSOLUTE path never loads its file.
#
# rubygems registers every one of its classes this way:
#
#     autoload :Dependency, File.expand_path("rubygems/dependency", __dir__)
#
# so the record holds `/…/gems/rubygems/lib/rubygems/dependency`. Touching
# `Gem::Dependency` raises `LoadError: cannot load such file -- <that path>`
# under zeo, and answers the class under ruby.
#
# WHAT IS RULED OUT, measured rather than assumed:
#   * The file is there. `File.exist?(path + ".rb")` is true.
#   * `require "rubygems/dependency"` -- the FEATURE spelling -- works, and
#     `Gem::Dependency` resolves immediately afterwards.
#   * `require <the same absolute path>` works when written in a program.
#   * `resolve_on_disk` handles a leading `/` and appends `.rb`, so path
#     resolution is not obviously the miss.
#
# So the failure is specific to the autoload ROUTE, not to the path, the file
# or the feature. `Module#const_missing` is what raises, per the backtrace, so
# the next step is that path rather than `run_pending_autoload`.
#
# A load-path-relative retry inside `run_pending_autoload` was tried and did
# NOT fix it, which is why this file says the diagnosis is unfinished instead
# of naming a cause.
#
# WHY IT MATTERS: it is the last thing between zeo and reading a real `.gem`.
# `Gem::Specification.from_yaml` revives the whole nested graph correctly --
# Specification, Version, Dependency, Requirement -- once those classes are
# loaded, which `tests/milestones/a_gemspec_loads_from_yaml.rb` proves by
# requiring them by hand. Every gem has dependencies, so nothing can open one
# until an autoload works on its own.
require "rubygems"

p Object.const_get("Gem::Dependency")
