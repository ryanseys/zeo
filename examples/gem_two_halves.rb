# A gem can have BOTH a native half and a Ruby half, joined by name -- the
# normal shape in Ruby, where digest, json, socket and strscan all ship C
# alongside lib/*.rb. spinel splits them by toolchain (Rust in spinel-rt,
# Ruby under gems/) exactly as CRuby splits them by install destination
# (archdir vs rubylibdir), and the Ruby half pulls its native half in with
# `require "<name>.so"` -- CRuby's own loader idiom.
#
# The payoff is exception classes: a feature-gated NATIVE class cannot
# register a constructible exception, so these used to degrade to RuntimeError.
# Defined in the Ruby half they are ordinary user classes, and the native half
# raises them by name.

require "json"
require "strscan"
require "monitor"

# The native half raises the Ruby half's exception class, by name.
begin
  JSON.parse("{oops")
rescue JSON::ParserError => e
  p e.class.name
  p e.class.ancestors.include?(StandardError)
end

p JSON::ParserError.superclass.name
p StringScanner::Error.ancestors.include?(StandardError)

# The native half still works through the Ruby half's require.
p JSON.dump({ "a" => 1 })
s = StringScanner.new("hello world")
p s.scan(/\w+/)
p s.rest

# MonitorMixin is pure Ruby over the native reentrant lock.
class Counter
  include MonitorMixin
  def initialize
    mon_initialize
    @n = 0
  end

  def bump
    synchronize { @n += 1 }
  end

  def reentrant
    synchronize { synchronize { mon_owned? } }
  end

  attr_reader :n
end

c = Counter.new
c.bump
c.bump
p c.n
p c.reentrant
p c.mon_owned?
