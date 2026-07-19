# RUBY_* version/build constants and the File::SEPARATOR family. The
# platform/description/revision are machine- and build-specific, so this
# checks the STABLE values exactly and the dynamic ones by self-consistency
# (that spinel composes RUBY_DESCRIPTION from its parts exactly as CRuby does),
# keeping the golden output portable across build hosts.
puts RUBY_VERSION
puts RUBY_ENGINE
puts RUBY_ENGINE_VERSION
puts RUBY_PATCHLEVEL
puts RUBY_RELEASE_DATE
puts File::SEPARATOR
p File::ALT_SEPARATOR
puts File::PATH_SEPARATOR

# Structural checks (true on every build host):
puts RUBY_PLATFORM.include?("-")
puts RUBY_REVISION.length >= 10
short = RUBY_REVISION[0, 10]
composed = "ruby #{RUBY_VERSION} (#{RUBY_RELEASE_DATE} revision #{short}) +PRISM [#{RUBY_PLATFORM}]"
puts RUBY_DESCRIPTION == composed
