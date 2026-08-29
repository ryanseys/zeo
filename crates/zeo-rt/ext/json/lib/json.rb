# The native half first -- CRuby's loader idiom (`ext/json/lib/json.rb` reaches
# its C parser and generator the same way, through `json/ext`).
require "json.so"

module JSON
  # The bundled gem's own version, which programs branch on.
  VERSION = "2.21.2"

  # Every `json/add/*.rb` opens by checking this before requiring `json`
  # itself, so it is what stops the additions from re-entering the library.
  JSON_LOADED = true

  # The three non-finite values the parser answers under `allow_nan:`, and
  # what a program compares a parsed result against.
  NaN = 0.0 / 0.0
  Infinity = 1.0 / 0.0
  MinusInfinity = -Infinity

  # CRuby's hierarchy (ext/json/lib/json/common.rb). Defined here in Ruby
  # rather than in the native half because a feature-gated native class
  # cannot register a constructible exception -- see
  # ext/strscan/lib/strscan.rb.
  #
  # `NestingError` sits under ParserError, so `rescue JSON::ParserError`
  # catches a too-deep parse -- and the GENERATOR raises it too, which is
  # what a cycle reports.
  class JSONError < StandardError; end
  class ParserError < JSONError; end
  class NestingError < ParserError; end
  class GeneratorError < JSONError; end
  class MissingUnicodeSupport < JSONError; end

  # The key an addition writes to name the class it came from.
  def self.create_id = "json_class"

  # Text spliced into the output VERBATIM, whatever the layout options say.
  # A fragment is not re-indented -- that is the point of it.
  class Fragment
    attr_reader :json

    def initialize(json)
      @json = json.to_s
    end

    def to_json(*) = @json
  end

  # An OpenStruct-alike the additions machinery revives an untagged object
  # into. Only the surface `json/common.rb` promises: read and write by
  # name, `[]`, and construction from a Hash.
  class GenericObject
    def initialize(hash = {})
      @table = {}
      hash.each { |k, v| @table[k.to_sym] = v }
    end

    def [](name) = @table[name.to_sym]
    def []=(name, value)
      @table[name.to_sym] = value
    end
    def to_h = @table.dup
    def to_json(state = nil, *) = JSON.generate(@table, state)

    def method_missing(name, *args)
      n = name.to_s
      return @table[n.chomp("=").to_sym] = args.first if n.end_with?("=")
      return @table[name] if @table.key?(name)

      super
    end

    def respond_to_missing?(name, include_private = false)
      @table.key?(name.to_s.chomp("=").to_sym) || super
    end
  end

  # The generator's option bag. CRuby's is a C struct with accessors; the
  # options travel to the native generator as an ordinary Hash, so this is a
  # plain object that remembers them and can generate with them.
  #
  # Named where CRuby's C extension names it, because `#class` and `#inspect`
  # say so -- `JSON::State` below is the alias every program actually writes.
  module Ext
    module Generator
      class State
        # `to_h`'s key order, which is the C struct's field order and what a
        # program comparing two states sees.
        ATTRS = %i[indent space space_before object_nl array_nl as_json allow_nan
                   ascii_only max_nesting script_safe strict depth
                   buffer_initial_length sort_keys].freeze

        # The plain readers. `allow_nan`, `ascii_only` and `strict` are asked with
        # a `?` instead -- CRuby gives those three no bare reader.
        attr_reader :indent, :space, :space_before, :object_nl, :array_nl,
                    :as_json, :max_nesting, :script_safe, :depth,
                    :buffer_initial_length, :sort_keys
        # `sort_keys=` and `as_json=` are written out below, because each stores
        # something other than what it was handed.
        attr_writer :indent, :space, :space_before, :object_nl, :array_nl,
                    :allow_nan, :ascii_only, :max_nesting, :script_safe, :strict,
                    :depth, :buffer_initial_length

        # What `to_json(state)` calls to normalize whatever it was handed: a
        # State passes through unchanged (identity, not a copy), a Hash becomes
        # one, and anything else -- `nil` included -- gets the defaults.
        # `json/add/symbol` reaches for it, so it is not optional.
        def self.from_state(opts)
          case opts
          when self then opts
          when Hash then new(opts)
          else new
          end
        end

        # `sort_keys = true` stores a TRANSFORM rather than the flag: the
        # generator hands each object to it and emits what comes back, so a
        # caller can supply any reordering it likes and the generator asks only
        # one question of either.
        DEFAULT_SORT = ->(hash) { hash.sort.to_h }

        def initialize(opts = {})
          @indent = @space = @space_before = @object_nl = @array_nl = ""
          @allow_nan = @ascii_only = @script_safe = @strict = false
          @as_json = @sort_keys = false
          @max_nesting = 100
          @depth = 0
          @buffer_initial_length = 1024
          configure(opts)
        end

        def configure(opts)
          (opts || {}).each { |k, v| self[k] = v }
          self
        end
        alias merge configure

        def allow_nan? = @allow_nan
        def ascii_only? = @ascii_only
        def script_safe? = @script_safe
        def strict? = @strict
        def strict = @strict

        # `escape_slash` is the old spelling of `script_safe`, and CRuby keeps
        # both names on the one flag.
        alias escape_slash script_safe
        alias escape_slash? script_safe?
        alias escape_slash= script_safe=

        # True whenever a nesting limit is in force -- `max_nesting: 0` turns the
        # limit off, and the circular check with it.
        def check_circular? = !@max_nesting.nil? && @max_nesting != 0

        # A Symbol or a bare `true` becomes the Proc the generator calls, so both
        # of these read back as one.
        def sort_keys=(v)
          @sort_keys = v == true ? DEFAULT_SORT : v
        end

        def as_json=(v)
          @as_json = v.is_a?(Symbol) ? v.to_proc : v
        end

        # An option the state does not model still round-trips: it lands in an
        # ivar of its own, which is where CRuby's `[]` falls back to as well. It
        # stays out of `to_h`, which lists the struct's own fields and nothing
        # else.
        def [](name)
          respond_to?(name) ? __send__(name) : instance_variable_get(:"@#{name}")
        end

        def []=(name, value)
          setter = :"#{name}="
          if respond_to?(setter)
            __send__(setter, value)
          else
            instance_variable_set(:"@#{name}", value)
          end
        end

        def to_h
          ATTRS.to_h { |a| [a, instance_variable_get(:"@#{a}")] }
        end
        alias to_hash to_h

        def generate(obj) = JSON.generate(obj, to_h)
      end
    end
  end

  State = Ext::Generator::State

  class << self
    # `dump(obj, anIO = nil, limit = nil)`. The second argument is
    # overloaded, and CRuby resolves it by TYPE: an Integer is the limit,
    # `nil` is nothing at all, and anything else is a port to write to. The
    # port is what comes back, which is what makes `JSON.dump(x, io)`
    # compose with a caller that keeps writing to it.
    def dump(obj, anIO = nil, limit = nil, **opts)
      if anIO.is_a?(Integer)
        limit = anIO
        anIO = nil
      end
      opts = opts.merge(max_nesting: limit) if limit
      text = begin
        generate(obj, opts)
      rescue JSON::NestingError
        raise ArgumentError, "exceed depth limit"
      end
      return text unless anIO

      anIO.write(text)
      anIO
    end
    alias unparse dump
    alias fast_unparse dump

    # `load(source, proc = nil, options = {})`. Looser than `parse`: a nil or
    # empty source answers nil, and the proc VISITS every parsed object,
    # innermost first.
    def load(source, proc = nil, options = {})
      source = source.read if source.respond_to?(:read)
      source = source.to_str if source.respond_to?(:to_str)
      return nil if source.nil? || source.empty?

      # `load` turns additions ON by default, so naming `symbolize_names`
      # conflicts with a default the caller never wrote.
      if options[:symbolize_names] && options.fetch(:create_additions, true)
        raise ArgumentError,
              "options :symbolize_names and :create_additions cannot be  used in conjunction"
      end

      # `load` is the addition-aware entry point: `create_additions` is ON
      # unless the caller turns it off, where `parse` leaves it off.
      defaults = { max_nesting: false, allow_nan: true, create_additions: true }
      result = parse(source, defaults.merge(options))
      recurse_proc(result, &proc) if proc
      result
    end
    alias restore load

    # Depth-first, children before their container -- the order CRuby's own
    # `recurse_proc` walks in.
    def recurse_proc(result, &proc)
      case result
      when Array then result.each { |x| recurse_proc(x, &proc) }
      when Hash then result.each { |k, v| recurse_proc(k, &proc); recurse_proc(v, &proc) }
      end
      proc.call(result)
    end

    # `generate` with every check off. CRuby's is a separate C entry; the
    # observable difference is the missing nesting limit.
    def fast_generate(obj, opts = nil)
      generate(obj, (opts || {}).merge(max_nesting: false))
    end
    alias fast_unparse fast_generate
  end
end

# `require "json"` gives every object a `#to_json`. CRuby installs these from
# the generator (`json/ext/generator`), which defines one per type it can encode
# directly, plus the `Object` catch-all from `json/common.rb`. The argument is
# the generator state CRuby threads through; nothing here needs it, but the
# parameter has to be accepted -- `obj.to_json(state)` is how a container asks
# its elements to encode themselves.
class Object
  # Anything with no JSON representation of its own encodes as its `to_s`, AS A
  # JSON STRING (`Object.new.to_json` is `"\"#<Object:0x...>\""`, not an
  # object) -- `JSON.generate(self)` would raise here instead.
  def to_json(*) = to_s.to_json
end

class Hash
  def to_json(state = nil, *) = JSON.generate(self, state)
end

class Array
  def to_json(state = nil, *) = JSON.generate(self, state)
end

class String
  def to_json(state = nil, *) = JSON.generate(self, state)
end

class Integer
  def to_json(state = nil, *) = JSON.generate(self, state)
end

class Float
  def to_json(state = nil, *) = JSON.generate(self, state)
end

# A Symbol encodes as its name, like a String -- the `json/add/symbol` form
# (`{"json_class":"Symbol",...}`) is opt-in and not loaded by a plain require.
class Symbol
  def to_json(state = nil, *) = JSON.generate(self, state)
end

class NilClass
  def to_json(*) = "null"
end

class TrueClass
  def to_json(*) = "true"
end

class FalseClass
  def to_json(*) = "false"
end
