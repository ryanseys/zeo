# What a store answers about itself, printed one gem per line.
#
# Both stores are read by the SAME engine, so a difference here is a
# difference in what the install WROTE -- not in what the reader makes of it.
# The bundle path is the only GEM_PATH, so nothing ambient can join the list.
#
# usage: dump_stubs.rb   (GEM_HOME/GEM_PATH name the store)
require "rubygems"

Gem::Specification.reset
Gem::Specification.stubs.sort_by { |s| [s.name, s.version.to_s, s.platform.to_s] }.each do |s|
  deps = s.dependencies.sort_by(&:name).map { |d| "#{d.name} #{d.type} #{d.requirement}" }
  puts [
    s.name,
    s.version,
    s.platform,
    s.executables.sort.join(","),
    s.require_paths.join(","),
    s.extensions.sort.join(","),
    deps.join(";"),
  ].join("\t")
end
