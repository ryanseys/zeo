# A class body compiled at RUN TIME is recovered from the snippet's own
# source text, and the slice has to start where the BODY starts -- not at
# the first statement.
#
# `lower::defs` flattens a `class << self` into a surrogate reopen plus the
# retagged `def`s, so neither the opener nor the closing `end` belongs to a
# statement any more. The recovery already added the closing `end` back. It
# did not add the OPENER, and with a constant inside the singleton body the
# first statement is the one written INSIDE it -- so the recovered text
# carried one `end` too many and prism refused the whole snippet with
# `unexpected \'end\'`, at a line the program never wrote.
#
# `rubygems/vendor/uri/lib/uri/common.rb` is written exactly that way, and
# this was the second thing standing between zeo and `require "rubygems"`.
#
# Slicing from the end of the class HEADER hands back the text the program
# wrote, openers included.

def try(name, src)
  eval src, nil, "s.rb"
  puts "#{name}\tok"
rescue Exception => e
  puts "#{name}\t#{e.class}: #{e.message.lines.first.chomp}"
end

# A constant inside `class << self` -- the shape that refused.
try("const in a singleton body", <<~SRC)
  module E1
    class << self
      CHARS = ".+-"
      def escape(s) = s.tr(CHARS, "___")
    end
  end
SRC
puts "escape\t#{E1.escape("a.b")}"

# A brace block spanning lines, which was never the problem but reads like it.
try("a multi-line brace block", <<~SRC)
  module E2
    class << self
      def pairs
        [1, 2].map { |x|
          [x, x]
        }.to_h
      end
    end
  end
SRC
puts "pairs\t#{E2.pairs}"

# Both together, plus a one-line header (`;` ends it, not a newline).
try("a one-line class header", <<~SRC)
  class E3; class << self
      LIMIT = 3
      def under = (1..LIMIT).to_a
    end
  end
SRC
puts "under\t#{E3.under}"

# A plain body still reports the same lines it always did.
try("a plain body raising", <<~SRC)
  module E4
    def self.boom
      raise "x"
    end
  end
SRC
begin
  E4.boom
rescue RuntimeError => e
  puts "frame\t#{e.backtrace.first}"
end
