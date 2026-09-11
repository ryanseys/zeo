# frozen_string_literal: true
module Psych
  module Visitors
    # Writes a finished node tree back out as YAML text.
    #
    # Upstream's Emitter feeds libyaml's event emitter, which zeo does not
    # carry. This one walks the tree directly. The output is valid YAML
    # that round-trips through any psych; libyaml's exact line wrapping
    # and style election are not reproduced, so byte-for-byte parity with
    # CRuby's output is not promised -- parse-level equality is.
    class Emitter
      def initialize(io, options = {})
        @io = io
      end

      def accept(target)
        case target
        when Psych::Nodes::Stream
          target.children.each_with_index { |doc, i| write_document(doc, i.zero?) }
        when Psych::Nodes::Document
          # libyaml's emitter must open a stream before a document.
          raise "expected STREAM-START"
        else
          # A bare node: wrap it the way a one-document stream would.
          doc = Psych::Nodes::Document.new([], [], true)
          doc.children << target
          write_document(doc, true)
        end
        target
      end

      private

      # One document. Its `%YAML`/`%TAG` directives lead it; the `---` start
      # marker is left off, as libyaml leaves it, only for an implicit
      # document that is first in its stream, has no directives, and whose
      # root carries no tag or anchor.
      def write_document(doc, first)
        @tag_directives = doc.tag_directives || []
        version = doc.version || []
        @io.write("%YAML #{version.join('.')}\n") unless version.empty?
        @tag_directives.each { |handle, prefix| @io.write("%TAG #{handle} #{prefix}\n") }
        root = doc.root
        directives = !version.empty? || !@tag_directives.empty?
        if doc.implicit && first && !directives && node_prefix(root).empty?
          write_implicit_root(root)
        else
          write_explicit_root(root)
        end
        @io.write("...\n") unless doc.implicit_end
      end

      def write_explicit_root(root)
        case root
        when Psych::Nodes::Scalar
          suffix = node_prefix(root)
          body = render_scalar(root)
          line = ["---", suffix, body].reject(&:empty?).join(" ")
          @io.write(line.rstrip + "\n")
        when Psych::Nodes::Alias
          @io.write("--- *#{root.anchor}\n")
        else
          head = node_prefix(root)
          if empty_collection?(root)
            @io.write(["---", head, flow(root)].reject(&:empty?).join(" ") + "\n")
          else
            @io.write(head.empty? ? "---\n" : "--- #{head}\n")
            write_block(root, 0)
          end
        end
      end

      # The root with no start marker: at column 0, a scalar or alias on its
      # own line and a collection as it would sit under `---`.
      def write_implicit_root(root)
        case root
        when Psych::Nodes::Scalar
          @io.write(render_scalar(root).rstrip + "\n")
        when Psych::Nodes::Alias
          @io.write("*#{root.anchor}\n")
        else
          if empty_collection?(root)
            @io.write(flow(root) + "\n")
          else
            write_block(root, 0)
          end
        end
      end

      # `!tag &anchor`, either half empty when absent.
      def node_prefix(node)
        tag = display_tag(node)
        anchor = node.respond_to?(:anchor) && node.anchor ? "&#{node.anchor}" : ""
        [tag, anchor].reject(&:empty?).join(" ")
      end

      # A tag as YAML spells it. YAMLTree already writes the surface form
      # (`!ruby/object:X`); a parsed tree may carry the canonical one.
      def display_tag(node)
        tag = node.tag
        return "" unless tag
        if node.is_a?(Psych::Nodes::Scalar) && node.plain && CANONICAL_SCALARS.include?(tag)
          # A plain scalar resolves its core type by itself.
          return ""
        end
        return tag if tag.start_with?("!")
        if (short = tag[/\Atag:yaml\.org,2002:(.*)\z/m, 1])
          return "!!#{short}"
        end
        if (short = tag[/\Atag:ruby\.yaml\.org,2002:(.*)\z/m, 1])
          return "!ruby/#{short}"
        end
        # A `%TAG` directive of this document shortens the tags it prefixes.
        (@tag_directives || []).each do |handle, prefix|
          return "#{handle}#{tag.delete_prefix(prefix)}" if tag.start_with?(prefix)
        end
        "!<#{tag}>"
      end

      CANONICAL_SCALARS = [
        "tag:yaml.org,2002:null", "tag:yaml.org,2002:bool",
        "tag:yaml.org,2002:int", "tag:yaml.org,2002:float",
        "tag:yaml.org,2002:str"
      ].freeze

      def empty_collection?(node)
        (node.is_a?(Psych::Nodes::Mapping) || node.is_a?(Psych::Nodes::Sequence)) &&
          node.children.empty?
      end

      def flow_style?(node)
        (node.is_a?(Psych::Nodes::Mapping) && node.style == Psych::Nodes::Mapping::FLOW) ||
          (node.is_a?(Psych::Nodes::Sequence) && node.style == Psych::Nodes::Sequence::FLOW)
      end

      # One node rendered inline -- scalars, aliases, and flow/empty
      # collections. Answers nil when the node needs block layout.
      def inline(node)
        case node
        when Psych::Nodes::Scalar
          prefixed(node, render_scalar(node))
        when Psych::Nodes::Alias
          "*#{node.anchor}"
        when Psych::Nodes::Mapping, Psych::Nodes::Sequence
          return prefixed(node, flow(node)) if empty_collection?(node) || flow_style?(node)
          nil
        end
      end

      def prefixed(node, body)
        head = node_prefix(node)
        head.empty? ? body : [head, body].reject(&:empty?).join(" ").rstrip
      end

      def flow(node)
        case node
        when Psych::Nodes::Mapping
          pairs = node.children.each_slice(2).map do |k, v|
            "#{inline(k) || flow(k)}: #{inline(v) || flow(v)}"
          end
          "{#{pairs.join(', ')}}"
        when Psych::Nodes::Sequence
          "[#{node.children.map { |c| inline(c) || flow(c) }.join(', ')}]"
        else
          inline(node)
        end
      end

      def write_block(node, indent)
        pad = " " * indent
        case node
        when Psych::Nodes::Mapping
          node.children.each_slice(2) do |k, v|
            key = inline(k) || flow(k)
            if (val = inline(v))
              # An empty plain scalar leaves nothing after the colon.
              @io.write(val.empty? ? "#{pad}#{key}:\n" : "#{pad}#{key}: #{val}\n")
            else
              head = node_prefix(v)
              @io.write(head.empty? ? "#{pad}#{key}:\n" : "#{pad}#{key}: #{head}\n")
              # psych's own convention: a sequence under a key sits at the
              # key's indent; a mapping nests one step in.
              child_indent = v.is_a?(Psych::Nodes::Sequence) ? indent : indent + 2
              write_block(v, child_indent)
            end
          end
        when Psych::Nodes::Sequence
          node.children.each do |c|
            if (val = inline(c))
              @io.write("#{pad}- #{val}\n".sub(/ +\n\z/, "\n"))
            else
              head = node_prefix(c)
              @io.write(head.empty? ? "#{pad}-\n" : "#{pad}- #{head}\n")
              write_block(c, indent + 2)
            end
          end
        else
          @io.write("#{pad}#{inline(node)}\n")
        end
      end

      def render_scalar(node)
        value = node.value.to_s
        style = node.style
        if style == Psych::Nodes::Scalar::SINGLE_QUOTED
          return "'#{value.gsub("'", "''")}'"
        end
        if style == Psych::Nodes::Scalar::DOUBLE_QUOTED
          return quote_double(value)
        end
        # LITERAL/FOLDED blocks and everything the plain form cannot carry
        # take the double-quoted spelling: always valid, never ambiguous.
        if node.plain && !node.quoted && plain_safe?(value)
          value
        elsif value.empty? || node.quoted && style == Psych::Nodes::Scalar::ANY
          value.include?("\n") ? quote_double(value) : "'#{value.gsub("'", "''")}'"
        else
          quote_double(value)
        end
      end

      def quote_double(value)
        out = +"\""
        value.each_char do |ch|
          out << case ch
                 when "\\" then "\\\\"
                 when "\"" then "\\\""
                 when "\n" then "\\n"
                 when "\t" then "\\t"
                 when "\r" then "\\r"
                 when "\0" then "\\0"
                 when "\e" then "\\e"
                 else
                   ch.ord < 0x20 ? format("\\x%02X", ch.ord) : ch
                 end
        end
        out << "\""
      end

      # Conservative: anything this does not obviously cover is quoted.
      # An empty plain scalar stays empty -- that is YAML's null spelling.
      def plain_safe?(value)
        return true if value.empty?
        return false if value != value.strip
        return false unless value =~ /\A[A-Za-z0-9_\-\.\/][A-Za-z0-9_\-\.\/ :=(),']*\z/
        return false if value.include?(": ") || value.include?(" #")
        return false if value.end_with?(":")
        true
      end
    end
  end
end
