# MILESTONE: a real `Gem::Specification` comes back out of YAML.
#
# This is the shape a `.gem` archive carries in its `metadata.gz`, and reading
# it is the single thing that stood between zeo and opening one. Before the
# `!ruby/object:` revival landed, the document loaded as a plain Hash and
# rubygems answered "YAML data doesn't evaluate to gem specification".
#
# The rung that makes it work is `init_with`: `Gem::Specification` defines one
# that forwards to its own `yaml_initialize`, so psych hands it a
# `Psych::Coder` and rubygems does the rest -- including coercing `date` and
# rebuilding `Gem::Version` from its own nested `!ruby/object:`.
#
# Shapes, never versions -- see `tests/milestones.rs`.

# Nothing is required but rubygems itself. `Gem::Version`, `Gem::Dependency`
# and `Gem::Requirement` all arrive through rubygems' own `autoload`, which is
# how a real caller meets them -- and which zeo could not run at all until the
# read path was given the chance to.
require "yaml"
require "rubygems"

YAML_TEXT = <<~Y
  --- !ruby/object:Gem::Specification
  name: widget
  version: !ruby/object:Gem::Version
    version: 1.2.3
  platform: ruby
  authors:
  - Someone
  date: 2026-08-28 00:00:00.000000000 Z
  dependencies:
  - !ruby/object:Gem::Dependency
    name: rake
    requirement: !ruby/object:Gem::Requirement
      requirements:
      - - ">="
        - !ruby/object:Gem::Version
          version: '0'
    type: :development
  summary: a widget
  require_paths:
  - lib
  files:
  - lib/widget.rb
Y

spec = Gem::Specification.from_yaml(YAML_TEXT)

puts spec.class
puts spec.name
puts spec.version.class
puts spec.version.to_s
puts spec.summary
puts spec.authors.inspect
puts spec.require_paths.inspect
puts spec.files.inspect
puts spec.platform.to_s

dep = spec.dependencies.first
puts dep.class
puts dep.name
puts dep.type.inspect
puts dep.requirement.to_s

# The nested revival went all the way down: every one of these is an object
# rather than the Hash it was written as.
puts spec.dependencies.map { |d| d.requirement.requirements.first.last.class }.inspect
__END__
Gem::Specification
widget
Gem::Version
1.2.3
a widget
["Someone"]
["lib"]
["lib/widget.rb"]
ruby
Gem::Dependency
rake
:development
>= 0
[Gem::Version]
