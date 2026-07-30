# frozen_string_literal: true
# :markup: markdown

# zeo: the zeo backend, adapted from upstream's `prism/ffi.rb`. Upstream's FFI
# backend reaches the C library through `dlopen`; zeo links that same library
# in and reaches it through the built-in `Prism::Zeo` module. Everything else
# is upstream's -- the option packing below is copied verbatim, so the option
# encoding stays defined in exactly one place, and `Prism::Serialize` decodes
# the buffers in Ruby just as it does for FFI.

module Prism
  # The version of the linked prism.
  VERSION = Zeo.version.freeze

  class << self
    # Mirror the Prism.dump API by using the serialization API.
    def dump(source, **options)
      dumped = Zeo.serialize_parse(source, dump_options(options))
      dumped.freeze if options.fetch(:freeze, false)
      dumped
    end

    # Mirror the Prism.dump_file API by using the serialization API.
    def dump_file(filepath, **options)
      options[:filepath] = filepath
      dump(File.binread(filepath), **options)
    end

    # Mirror the Prism.lex API by using the serialization API.
    def lex(code, **options)
      Serialize.load_lex(code, Zeo.serialize_lex(code, dump_options(options)), options.fetch(:freeze, false))
    end

    # Mirror the Prism.lex_file API by using the serialization API.
    def lex_file(filepath, **options)
      options[:filepath] = filepath
      lex(File.binread(filepath), **options)
    end

    # Mirror the Prism.parse API by using the serialization API.
    def parse(code, **options)
      Serialize.load_parse(code, Zeo.serialize_parse(code, dump_options(options)), options.fetch(:freeze, false))
    end

    # Mirror the Prism.parse_file API by using the serialization API.
    def parse_file(filepath, **options)
      options[:filepath] = filepath
      parse(File.binread(filepath), **options)
    end

    # Mirror the Prism.parse_stream API by using the serialization API.
    def parse_stream(stream, **options)
      source = +""
      result = nil

      while (line = stream.gets)
        source << line
        result = parse(source, **options)
        break if result.success?
      end

      result || parse(source, **options)
    end

    # Mirror the Prism.parse_comments API by using the serialization API.
    def parse_comments(code, **options)
      Serialize.load_parse_comments(code, Zeo.serialize_parse_comments(code, dump_options(options)), options.fetch(:freeze, false))
    end

    # Mirror the Prism.parse_file_comments API by using the serialization API.
    def parse_file_comments(filepath, **options)
      options[:filepath] = filepath
      parse_comments(File.binread(filepath), **options)
    end

    # Mirror the Prism.parse_lex API by using the serialization API.
    def parse_lex(code, **options)
      Serialize.load_parse_lex(code, Zeo.serialize_parse_lex(code, dump_options(options)), options.fetch(:freeze, false))
    end

    # Mirror the Prism.parse_lex_file API by using the serialization API.
    def parse_lex_file(filepath, **options)
      options[:filepath] = filepath
      parse_lex(File.binread(filepath), **options)
    end

    # Mirror the Prism.parse_success? API by using the serialization API.
    def parse_success?(code, **options)
      Zeo.parse_success?(code, dump_options(options))
    end

    # Mirror the Prism.parse_failure? API by using the serialization API.
    def parse_failure?(code, **options)
      !parse_success?(code, **options)
    end

    # Mirror the Prism.parse_file_success? API by using the serialization API.
    def parse_file_success?(filepath, **options)
      options[:filepath] = filepath
      parse_success?(File.binread(filepath), **options)
    end

    # Mirror the Prism.parse_file_failure? API by using the serialization API.
    def parse_file_failure?(filepath, **options)
      !parse_file_success?(filepath, **options)
    end

    # Mirror the Prism.profile API by using the serialization API.
    def profile(source, **options)
      Zeo.serialize_parse(source, dump_options(options))
      nil
    end

    # Mirror the Prism.profile_file API by using the serialization API.
    def profile_file(filepath, **options)
      options[:filepath] = filepath
      profile(File.binread(filepath), **options)
    end

    private

    # Return the value that should be dumped for the command_line option.
    def dump_options_command_line(options)
      command_line = options.fetch(:command_line, "")
      raise ArgumentError, "command_line must be a string" unless command_line.is_a?(String)

      command_line.each_char.inject(0) do |value, char|
        case char
        when "a" then value | 0b000001
        when "e" then value | 0b000010
        when "l" then value | 0b000100
        when "n" then value | 0b001000
        when "p" then value | 0b010000
        when "x" then value | 0b100000
        else raise ArgumentError, "invalid command_line option: #{char}"
        end
      end
    end

    # Return the value that should be dumped for the version option.
    def dump_options_version(version)
      current = version == "current"

      case current ? RUBY_VERSION : version
      when nil, "latest"
        0 # Handled in pm_parser_init
      when /\A3\.3(\.\d+)?\z/
        1
      when /\A3\.4(\.\d+)?\z/
        2
      when /\A3\.5(\.\d+)?\z/, /\A4\.0(\.\d+)?\z/
        3
      when /\A4\.1(\.\d+)?\z/
        4
      else
        if current
          raise CurrentVersionError, RUBY_VERSION
        else
          raise ArgumentError, "invalid version: #{version}"
        end
      end
    end

    # Convert the given options into a serialized options string.
    def dump_options(options)
      template = +""
      values = []

      template << "L"
      if (filepath = options[:filepath])
        values.push(filepath.bytesize, filepath.b)
        template << "A*"
      else
        values << 0
      end

      template << "l"
      values << options.fetch(:line, 1)

      template << "L"
      if (encoding = options[:encoding])
        name = encoding.is_a?(Encoding) ? encoding.name : encoding
        values.push(name.bytesize, name.b)
        template << "A*"
      else
        values << 0
      end

      template << "C"
      values << (options.fetch(:frozen_string_literal, false) ? 1 : 0)

      template << "C"
      values << dump_options_command_line(options)

      template << "C"
      values << dump_options_version(options[:version])

      template << "C"
      values << (options[:encoding] == false ? 1 : 0)

      template << "C"
      values << (options.fetch(:main_script, false) ? 1 : 0)

      template << "C"
      values << (options.fetch(:partial_script, false) ? 1 : 0)

      template << "C"
      values << (options.fetch(:freeze, false) ? 1 : 0)

      template << "L"
      if (scopes = options[:scopes])
        values << scopes.length

        scopes.each do |scope|
          locals = nil
          forwarding = 0

          case scope
          when Array
            locals = scope
          when Scope
            locals = scope.locals

            scope.forwarding.each do |forward|
              case forward
              when :*     then forwarding |= 0x1
              when :**    then forwarding |= 0x2
              when :&     then forwarding |= 0x4
              when :"..." then forwarding |= 0x8
              else raise ArgumentError, "invalid forwarding value: #{forward}"
              end
            end
          else
            raise TypeError, "wrong argument type #{scope.class.inspect} (expected Array or Prism::Scope)"
          end

          template << "L"
          values << locals.length

          template << "C"
          values << forwarding

          locals.each do |local|
            name = local.name
            template << "L"
            values << name.bytesize

            template << "A*"
            values << name.b
          end
        end
      else
        values << 0
      end

      values.pack(template)
    end
  end
end
