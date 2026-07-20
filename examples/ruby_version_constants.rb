# RUBY_* version/build constants and the File::SEPARATOR family that spinel
# shares with its ruby 4.0.5 oracle. The platform/revision are machine- and
# build-specific, so this checks the STABLE shared values exactly and the
# dynamic ones structurally, keeping the golden output portable across build
# hosts AND identical between spinel and the oracle.
#
# The ENGINE-IDENTITY constants (RUBY_ENGINE, RUBY_ENGINE_VERSION,
# RUBY_DESCRIPTION) mirror CRuby's own ("ruby", RUBY_VERSION, version.c's
# banner). Their exact values are asserted in `ruby_engine_identifies_as_ruby`;
# this golden covers the shared version/build constants.
puts RUBY_VERSION
puts RUBY_PATCHLEVEL
puts RUBY_RELEASE_DATE
puts File::SEPARATOR
p File::ALT_SEPARATOR
puts File::PATH_SEPARATOR

# Structural checks (true on every build host, both engines):
puts RUBY_PLATFORM.include?("-")
puts RUBY_REVISION.length >= 10
