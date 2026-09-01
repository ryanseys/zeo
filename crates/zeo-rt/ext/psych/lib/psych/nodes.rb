# The parse tree `Psych.parse` hands back.
#
# These are plain Ruby classes, exactly as upstream psych has them, because a
# program is allowed to BUILD one: `Psych::Nodes::Scalar.new("x")` then
# `to_ruby` is how a generator emits YAML without writing text. The Rust half
# constructs the same classes when it parses, and reads them back when
# `to_ruby` is called -- so a hand-built tree and a parsed one go through one
# walk.
#
# `#yaml`/`#to_yaml` emit a tree back to text through
# `Psych::Visitors::Emitter` (visitors/emitter.rb) -- valid, round-trippable
# YAML; libyaml's exact wrapping and style election are not reproduced.

require "stringio"

module Psych
  module Nodes
    # The shared half: children, a tag, and where the node was written.
    #
    # The four position readers exist and all answer 0: nothing in the load
    # path reads them, so the tree does not carry marks yet. See
    # tests/gaps/a_psych_scalar_node_knows_where_it_ends.rb, which records
    # what filling them needs and the one hard part.
    class Node
      include Enumerable

      attr_accessor :children, :tag
      attr_accessor :start_line, :start_column, :end_line, :end_column

      def initialize
        @children = []
        @tag = nil
        @start_line = @start_column = @end_line = @end_column = 0
      end

      # Depth first, CHILDREN BEFORE SELF. Upstream reaches this order through
      # `Visitors::DepthFirst`, and it is the order a rewriting visitor needs:
      # a node is handed over only once everything inside it has been.
      # Measured against ruby 4.0.6 -- `Psych.parse("a: [1]").each` yields
      # Scalar, Scalar, Sequence, Mapping, Document.
      def each(&block)
        return enum_for(:each) unless block_given?

        @children.each { |c| c.each(&block) }
        yield self
        self
      end

      def to_ruby(symbolize_names: false, freeze: false, strict_integer: false)
        Psych.__node_to_ruby(self,
                             symbolize_names: symbolize_names,
                             freeze: freeze,
                             strict_integer: strict_integer)
      end
      alias transform to_ruby

      def yaml(io = nil, options = {})
        real_io = io || StringIO.new(+"")
        Psych::Visitors::Emitter.new(real_io, options).accept(self)
        return real_io.string unless io
        io
      end
      alias to_yaml yaml
    end

    # A whole stream: one child per document.
    class Stream < Node
      # libyaml's encoding enum, the values upstream re-exports from
      # Psych::Parser. Spelled numerically: zeo has no Parser class, and
      # the one consumer (YAMLTree's start_stream) only stores the value.
      ANY = 0
      UTF8 = 1
      UTF16LE = 2
      UTF16BE = 3

      def initialize(encoding = Psych::Parser::UTF8)
        super()
        @encoding = encoding
      end
      attr_accessor :encoding
    end

    # One document, with the header and footer it was written with.
    class Document < Node
      # `[major, minor]`, or `[]` when the document named no `%YAML`.
      attr_accessor :version
      # `[[handle, prefix], ...]` from `%TAG`.
      attr_accessor :tag_directives
      # True when the document wrote no `---` / no `...` of its own.
      attr_accessor :implicit, :implicit_end

      def initialize(version = [], tag_directives = [], implicit = false)
        super()
        @version = version
        @tag_directives = tag_directives
        @implicit = implicit
        @implicit_end = true
      end

      # A document holds exactly one root.
      def root
        @children.first
      end
    end

    class Sequence < Node
      ANY = 0
      BLOCK = 1
      FLOW = 2

      attr_accessor :anchor, :implicit, :style

      def initialize(anchor = nil, tag = nil, implicit = true, style = BLOCK)
        super()
        @anchor = anchor
        @tag = tag
        @implicit = implicit
        @style = style
      end
    end

    class Mapping < Node
      ANY = 0
      BLOCK = 1
      FLOW = 2

      attr_accessor :anchor, :implicit, :style

      def initialize(anchor = nil, tag = nil, implicit = true, style = BLOCK)
        super()
        @anchor = anchor
        @tag = tag
        @implicit = implicit
        @style = style
      end
    end

    class Scalar < Node
      ANY = 0
      PLAIN = 1
      SINGLE_QUOTED = 2
      DOUBLE_QUOTED = 3
      LITERAL = 4
      FOLDED = 5

      attr_accessor :value, :anchor, :plain, :quoted, :style

      def initialize(value, anchor = nil, tag = nil, plain = true, quoted = false, style = ANY)
        super()
        @value = value
        @anchor = anchor
        @tag = tag
        @plain = plain
        @quoted = quoted
        @style = style
      end
    end

    # A `*name` reference. It has no children of its own.
    class Alias < Node
      attr_accessor :anchor

      def initialize(anchor = nil)
        super()
        @anchor = anchor
      end
    end
  end

  # `Psych::Parser::UTF8` and friends, which `Nodes::Stream` defaults to.
  # Upstream puts them on the parser class; nothing else of that class is
  # modelled, so this is the constant surface and not the parser.
  class Parser
    ANY = 0
    UTF8 = 1
    UTF16LE = 2
    UTF16BE = 3
  end
end
