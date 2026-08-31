# What a store answers about itself, printed one gem per line.
#
# Both stores are read by the SAME engine, so a difference here is a
# difference in what the install WROTE -- not in what the reader makes of it.
# The bundle path is the only GEM_PATH, so nothing ambient can join the list.
#
# Every stub is materialised with `to_spec`, which LOADS the `.gemspec` the
# install wrote. That is the point: a store whose specification files do not
# evaluate is a store that only looks right on disk.
#
# usage: dump_stubs.rb   (GEM_HOME/GEM_PATH name the store)
require "rubygems"

Gem::Specification.reset
Gem::Specification.stubs.sort_by { |s| [s.name, s.version.to_s, s.platform.to_s] }.each do |stub|
  s = stub.to_spec
  deps = s.dependencies.sort_by(&:name).map { |d| "#{d.name} #{d.type} #{d.requirement}" }
  puts [
    s.name,
    s.version,
    s.platform,
    s.executables.sort.join(","),
    s.require_paths.join(","),
    s.extensions.sort.join(","),
    s.required_ruby_version,
    s.licenses.sort.join(","),
    s.files.sort.join(","),
    deps.join(";"),
  ].join("\t")
end
