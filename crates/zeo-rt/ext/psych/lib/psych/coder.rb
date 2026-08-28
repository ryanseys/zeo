# What an object's `init_with` is handed when psych revives it.
#
# A class that defines `init_with(coder)` is saying "I will rebuild myself
# from the document" -- so the coder carries the tag it was written with and
# the mapping it was written as, and the class reads what it wants. This is
# the half of psych that makes `!ruby/object:Gem::Specification` come back as
# a specification rather than as a Hash.
#
# The `represent_*` writers exist because one coder serves both directions
# upstream: `encode_with(coder)` fills the same object on the way OUT. zeo's
# emitter does not consult a coder yet, so those writers record and nothing
# reads them -- tracked in tests/gaps/a_psych_coder_drives_encode_with.rb.

module Psych
  class Coder
    attr_accessor :tag, :style, :implicit, :object
    attr_reader :type, :seq

    def initialize(tag)
      @tag = tag
      @map = {}
      @seq = []
      @implicit = false
      @type = :map
      @style = Psych::Nodes::Mapping::BLOCK
      @scalar = nil
      @object = nil
    end

    def map=(map)
      @type = :map
      @map = map
    end

    def map(tag = @tag, style = @style)
      @tag = tag
      @style = style
      @type = :map
      @map
    end

    def scalar=(value)
      @type = :scalar
      @scalar = value
    end

    def scalar(*args)
      # `scalar()` reads; `scalar(tag, value, style)` writes. Upstream keeps
      # both spellings on one name, and gems use each.
      return @scalar if args.empty?

      @tag, @scalar, @style = args
      @type = :scalar
      @scalar
    end

    def seq=(list)
      @type = :seq
      @seq = list
    end

    def represent_scalar(tag, value)
      self.tag = tag
      self.scalar = value
    end

    def represent_seq(tag, list)
      @tag = tag
      self.seq = list
    end

    def represent_map(tag, map)
      @tag = tag
      self.map = map
    end

    # `represent_object(tag, obj)` says "dump THAT instead of me", which is
    # how a delegating class hands off.
    def represent_object(tag, obj)
      @tag = tag
      @type = :object
      @object = obj
    end

    def [](key)
      @map[key]
    end

    def []=(key, value)
      @type = :map
      @map[key] = value
    end
    alias add []=
  end
end
