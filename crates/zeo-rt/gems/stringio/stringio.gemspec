# frozen_string_literal: true

# The zeo-authored pure-Ruby StringIO port. Matches no published release:
# ruby's stringio gem is a C extension with no pure implementation, so this
# tree lives in zeo's own tier rather than riding the lock. The version
# tracks the locked C gem it is held against.
Gem::Specification.new do |s|
  s.name = "stringio"
  s.version = "3.2.0"
  s.summary = "Pure-Ruby StringIO, compiled by zeo"
  s.authors = ["zeo"]
  s.files = ["lib/stringio.rb"]
  s.require_paths = ["lib"]
end
