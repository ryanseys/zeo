# The ffi gem's Ruby-side surface. The native half comes first (CRuby's
# loader idiom), so the FFI module and its native classes exist to be
# extended below.
require "ffi.so"
require "rbconfig"

module FFI
  VERSION = "1.17.4"

  # The gem's exception hierarchy, and the three superclasses are not
  # decoration: `rescue LoadError` around a `require` of an FFI wrapper is
  # what catches a missing library or a missing symbol, and a NULL
  # dereference is a `RuntimeError` because it is a bug in the caller rather
  # than a load failure. Defined here rather than natively because a
  # feature-gated native class can't register a constructible exception (see
  # `crates/zeo-rt/src/ext/mod.rs`); as ordinary user classes they are
  # raisable by name from the native half (`FFI::NullPointerError` guards
  # every NULL read/write).
  class NullPointerError < RuntimeError; end
  class NotFoundError < LoadError; end
  class PlatformError < LoadError; end

  # glibc's soname, which the gem's C half defines only where it applies.
  # `Platform::IS_GNU` reads it through `defined?`, so on every other host
  # the constant's absence IS the answer.
  GNU_LIBC = "libc.so.6" if RbConfig::CONFIG["host_os"].include?("gnu")

  # The platform facts, derived from the same `RbConfig` entries the gem
  # derives them from -- which zeo bakes from the build target, so these
  # agree with the values `guard_fold` folds a `FFI::Platform.mac?` or a
  # `FFI::Platform::ADDRESS_SIZE == 64` guard against at compile time.
  module Platform
    OS = case RbConfig::CONFIG["host_os"].downcase
         when /linux/ then "linux"
         when /darwin/ then "darwin"
         when /freebsd/ then "freebsd"
         when /netbsd/ then "netbsd"
         when /openbsd/ then "openbsd"
         when /dragonfly/ then "dragonflybsd"
         when /sunos|solaris/ then "solaris"
         when /mingw|mswin/ then "windows"
         else RbConfig::CONFIG["host_os"].downcase
         end.freeze

    OSVERSION = RbConfig::CONFIG["host_os"].gsub(/[^\d]/, "").to_i

    CPU = RbConfig::CONFIG["host_cpu"].freeze

    ARCH = case CPU.downcase
           when /amd64|x86_64|x64/ then "x86_64"
           when /i\d86|x86|i86pc/ then "i386"
           when /ppc64|powerpc64/ then "powerpc64"
           when /ppc|powerpc/ then "powerpc"
           when /sparcv9|sparc64/ then "sparcv9"
           # macOS calls it arm64, every other OS aarch64.
           when /arm64|aarch64/ then "aarch64"
           when /^arm/ then OS == "darwin" ? "aarch64" : "arm"
           else RbConfig::CONFIG["host_cpu"]
           end.freeze

    def self.is_os(os)
      OS == os
    end

    IS_GNU = defined?(GNU_LIBC)
    IS_LINUX = is_os("linux")
    IS_MAC = is_os("darwin")
    IS_FREEBSD = is_os("freebsd")
    IS_NETBSD = is_os("netbsd")
    IS_OPENBSD = is_os("openbsd")
    IS_DRAGONFLYBSD = is_os("dragonfly")
    IS_SOLARIS = is_os("solaris")
    IS_WINDOWS = is_os("windows")
    IS_BSD = IS_MAC || IS_FREEBSD || IS_NETBSD || IS_OPENBSD || IS_DRAGONFLYBSD

    # FreeBSD 12 widened `ino_t`, so the gem carries the version in the name
    # for that one ABI break.
    name_version = "12" if IS_FREEBSD && OSVERSION >= 12

    NAME = "#{ARCH}-#{OS}#{name_version}".freeze
    CONF_DIR = File.join(File.dirname(__FILE__), "platform", NAME).freeze

    LIBPREFIX = (case OS
                 when /windows|msys/ then ""
                 when /cygwin/ then "cyg"
                 else "lib"
                 end).freeze

    LIBSUFFIX = (case OS
                 when /darwin/ then "dylib"
                 when /linux|bsd|solaris/ then "so"
                 when /windows|cygwin|msys/ then "dll"
                 else "so" # punt and assume a sane unix (i.e. anything but AIX)
                 end).freeze

    LIBC = (if IS_WINDOWS
              "ucrtbase.dll"
            elsif IS_GNU
              GNU_LIBC
            elsif OS == "cygwin"
              "cygwin1.dll"
            elsif OS == "msys"
              "msys-2.0.dll"
            else
              "#{LIBPREFIX}c.#{LIBSUFFIX}"
            end).freeze

    LITTLE_ENDIAN = 1234
    BIG_ENDIAN = 4321
    BYTE_ORDER = [0x12345678].pack("I") == [0x12345678].pack("N") ? BIG_ENDIAN : LITTLE_ENDIAN

    # The C-ABI widths, in BITS. The gem's C half reports what its compiler
    # measured; these read the same numbers back off the canonical
    # `FFI::Type` instances, which zeo's marshaling already sizes from the
    # target's own C ABI.
    ADDRESS_SIZE = Type::Builtin::POINTER.size * 8
    ADDRESS_ALIGN = Type::Builtin::POINTER.alignment * 8
    LONG_SIZE = Type::Builtin::LONG.size * 8
    LONG_ALIGN = Type::Builtin::LONG.alignment * 8
    INT8_SIZE = Type::Builtin::INT8.size * 8
    INT8_ALIGN = Type::Builtin::INT8.alignment * 8
    INT16_SIZE = Type::Builtin::INT16.size * 8
    INT16_ALIGN = Type::Builtin::INT16.alignment * 8
    INT32_SIZE = Type::Builtin::INT32.size * 8
    INT32_ALIGN = Type::Builtin::INT32.alignment * 8
    INT64_SIZE = Type::Builtin::INT64.size * 8
    INT64_ALIGN = Type::Builtin::INT64.alignment * 8
    FLOAT_SIZE = Type::Builtin::FLOAT.size * 8
    FLOAT_ALIGN = Type::Builtin::FLOAT.alignment * 8
    DOUBLE_SIZE = Type::Builtin::DOUBLE.size * 8
    DOUBLE_ALIGN = Type::Builtin::DOUBLE.alignment * 8
    # zeo has no `:long_double` type to measure, so these come from the ABI
    # rule instead. Every target zeo builds for gives `long double` a 16-byte
    # slot -- 80-bit extended on x86_64, IEEE quad on aarch64 SysV -- with one
    # exception: Apple aliases it to `double` on arm64 ONLY. An Intel Mac
    # answers 128.
    LONG_DOUBLE_SIZE = IS_MAC && ARCH == "aarch64" ? 64 : 128
    LONG_DOUBLE_ALIGN = LONG_DOUBLE_SIZE

    # Test if current OS is a *BSD (includes Mac).
    def self.bsd?
      IS_BSD
    end

    def self.windows?
      IS_WINDOWS
    end

    def self.mac?
      IS_MAC
    end

    def self.solaris?
      IS_SOLARIS
    end

    # The gem's own rule: everything that is not windows.
    def self.unix?
      !IS_WINDOWS
    end
  end

  # `Ractor.make_shareable`, which the gem hands its own frozen constants
  # through and exposes for a caller building shareable FFI state.
  def self.make_shareable(obj)
    Ractor.make_shareable(obj)
  end

  # The stand-in a library list uses for "search this process", rather than
  # a library to dlopen. Two names for one object, as the gem has.
  CURRENT_PROCESS = USE_THIS_PROCESS_AS_LIBRARY = make_shareable(Object.new)

  # A generic library name plus an optional ABI number, rendered into the
  # platform's own file name -- `LibraryPath.new("vips", abi_number: 42)` is
  # `libvips.42.dylib` on macOS, `libvips.so.42` on linux and
  # `libvips-42.dll` on windows.
  class LibraryPath
    attr_reader :name, :abi_number, :root

    def initialize(name, abi_number: nil, root: nil)
      @name = name
      @abi_number = abi_number
      @root = root
    end

    def self.wrap(value)
      return value if value.is_a?(self)
      # 'c' names the standard C library rather than a file called `libc`.
      return Library::LIBC if value == "c"
      # A bare file name becomes a library path; anything carrying a
      # directory is already a full path to a library.
      return new(value) if value && File.basename(value) == value

      value
    end

    def full_name
      if abi_number
        if Platform.windows?
          "#{Platform::LIBPREFIX}#{name}-#{abi_number}.#{Platform::LIBSUFFIX}"
        elsif Platform.mac?
          "#{Platform::LIBPREFIX}#{name}.#{abi_number}.#{Platform::LIBSUFFIX}"
        else
          "#{Platform::LIBPREFIX}#{name}.#{Platform::LIBSUFFIX}.#{abi_number}"
        end
      else
        lib = name
        lib = Platform::LIBPREFIX + lib unless lib =~ /^#{Platform::LIBPREFIX}/
        # A linux soname keeps its version suffix (`libz.so.1`), so the
        # extension test there accepts a trailing number.
        r = if Platform.windows? || Platform.mac?
              "\\.#{Platform::LIBSUFFIX}$"
            else
              "\\.so($|\\.[1234567890]+)"
            end
        lib += ".#{Platform::LIBSUFFIX}" unless lib =~ /#{r}/
        lib
      end
    end

    def to_s
      root ? File.join(root, full_name) : full_name
    end
  end

  # `jpeg` -> `libjpeg.dylib`. A full path passes through untouched.
  def self.map_library_name(lib)
    LibraryPath.wrap(lib).to_s
  end

  # Every canonical type under a second set of names. `NativeType` is the
  # module the gem's own code reaches through, and `FFI::TYPE_INT32` the
  # top-level spelling -- all three name ONE object per type.
  module NativeType
    Type::Builtin.constants.each do |name|
      t = Type::Builtin.const_get(name)
      # Only the canonical names, which are the ones `#inspect` prints.
      const_set(name, t) if t.is_a?(Type) && t.inspect.include?("::#{name} ")
    end
  end

  NativeType.constants.each { |name| const_set("TYPE_#{name}", NativeType.const_get(name)) }

  # The type each keyword spells. The gem's C half also loads the host's
  # `types.conf` here (`:__darwin_ino_t` and some hundreds of siblings);
  # zeo resolves those at COMPILE time instead -- see `CScalar::from_c_typedef`
  # -- so they are absent from this runtime table rather than answered with a
  # width measured on some other machine.
  TypeDefs = {
    void: Type::Builtin::VOID,
    bool: Type::Builtin::BOOL,
    string: Type::Builtin::STRING,
    char: Type::Builtin::CHAR,
    uchar: Type::Builtin::UCHAR,
    short: Type::Builtin::SHORT,
    ushort: Type::Builtin::USHORT,
    int: Type::Builtin::INT,
    uint: Type::Builtin::UINT,
    long: Type::Builtin::LONG,
    ulong: Type::Builtin::ULONG,
    long_long: Type::Builtin::LONG_LONG,
    ulong_long: Type::Builtin::ULONG_LONG,
    float: Type::Builtin::FLOAT,
    double: Type::Builtin::DOUBLE,
    long_double: Type::Builtin::LONGDOUBLE,
    pointer: Type::Builtin::POINTER,
    int8: Type::Builtin::INT8,
    uint8: Type::Builtin::UINT8,
    int16: Type::Builtin::INT16,
    uint16: Type::Builtin::UINT16,
    int32: Type::Builtin::INT32,
    uint32: Type::Builtin::UINT32,
    int64: Type::Builtin::INT64,
    uint64: Type::Builtin::UINT64,
    buffer_in: Type::Builtin::BUFFER_IN,
    buffer_out: Type::Builtin::BUFFER_OUT,
    buffer_inout: Type::Builtin::BUFFER_INOUT,
    varargs: Type::Builtin::VARARGS,
  }

  # `FFI.typedef` writes here rather than into `TypeDefs`, which is what
  # CRuby does too (the C half keeps its own lookup table and leaves the
  # published constant alone). Reflection only: an `attach_function`
  # signature naming an alias resolves it at compile time, and a `typedef`
  # the compiler cannot fold is a compile error rather than a wrong width.
  @custom_typedefs = {}

  def self.find_type(name, type_map = nil)
    return name if name.is_a?(Type)

    t = type_map && type_map[name]
    t ||= @custom_typedefs[name] || TypeDefs[name]
    raise TypeError, "unable to resolve type '#{name}'" unless t

    t
  end

  def self.add_typedef(old, add)
    @custom_typedefs[add] = find_type(old)
  end

  def self.typedef(old, add)
    add_typedef(old, add)
  end

  def self.type_size(type)
    find_type(type).size
  end

  # The non-scalar type descriptors, and the layout a struct class answers
  # from `.layout`. zeo builds all of these from the layout its compiler
  # already walked, so they describe exactly the bytes the accessors read.
  #
  # They are `FFI::Type` subclasses as CRuby's are, so each carries the
  # builtin type constants and answers `is_a?(FFI::Type)`; only
  # `StructLayout::Field` is `< Object`, in CRuby too.
  #
  # DIVERGENCE: a `:long` field reports `Type::Builtin::INT64` rather than
  # `LONG`, because the compiler folds the two to one width before a layout
  # is recorded.
  class ArrayType < Type
    attr_reader :elem_type, :length

    def initialize(elem_type, length)
      @elem_type = elem_type
      @length = length
    end

    def size = @elem_type.size * @length
    def alignment = @elem_type.alignment

    def inspect
      format("#<FFI::ArrayType::0x%016x size=%d alignment=%d>", object_id, size, alignment)
    end
  end

  class StructByValue < Type
    attr_reader :struct_class

    def initialize(struct_class) = @struct_class = struct_class
    def size = @struct_class.size
    def alignment = @struct_class.alignment

    def inspect
      format("#<FFI::StructByValue::0x%016x size=%d alignment=%d>", object_id, size, alignment)
    end
  end

  class FunctionType < Type
    attr_reader :param_types, :result_type

    def initialize(result_type, param_types)
      @result_type = result_type
      @param_types = param_types
    end

    def size = Type::Builtin::POINTER.size
    def alignment = Type::Builtin::POINTER.alignment

    def inspect
      format("#<FFI::FunctionType::0x%016x size=%d alignment=%d>", object_id, size, alignment)
    end
  end

  # The gem names `Type::Function` twice more; the descriptor classes above
  # are the canonical names (`FFI::ArrayType` and friends), as in the gem.
  CallbackInfo = FunctionType

  # Qualified on purpose: writing these inside a `class Type` body makes the
  # compiler's FFI prescan misroute this file's later `class Struct` reopen
  # (`layout` and friends vanish). Recorded in the overhaul plan backlog.
  Type::Array = FFI::ArrayType
  Type::Function = FFI::FunctionType
  Type::Struct = FFI::StructByValue

  class Type
    # A type that converts on the way in and out -- what an `enum` field's
    # descriptor is. The conversion itself lives in the struct's generated
    # accessor; this records the native type underneath it.
    class Mapped < Type
      attr_reader :native_type

      def initialize(native_type) = @native_type = native_type
      def size = @native_type.size
      def alignment = @native_type.alignment

      def inspect
        format("#<FFI::Type::Mapped::0x%016x size=%d alignment=%d>", object_id, size, alignment)
      end
    end
  end

  class StructLayout < Type
    class Field
      attr_reader :name, :offset, :type, :size, :alignment

      def initialize(name, offset, type, size, alignment, owner)
        @name = name
        @offset = offset
        @type = type
        @size = size
        @alignment = alignment
        @owner = owner
      end

      # A field's read/write against a pointer to the struct's FIRST byte --
      # so it goes through a struct viewing those bytes, which is where every
      # conversion (an enum's symbol, an inline array's proxy, a nested
      # struct's view) already lives.
      def get(ptr) = @owner.new(ptr)[@name]
      def put(ptr, value) = @owner.new(ptr)[@name] = value
    end

    # The Field subclass names CRuby reports, one per storage shape.
    class Number < Field; end
    class Pointer < Field; end
    class String < Field; end
    class Array < Field; end
    class InnerStruct < Field; end
    class Function < Field; end
    class Mapped < Field; end
    class Enum < Field; end

    attr_reader :fields, :size, :alignment

    def initialize(fields, size, alignment)
      @fields = fields
      @size = size
      @alignment = alignment
    end

    def members = @fields.map(&:name)
    def offsets = @fields.map { |f| [f.name, f.offset] }
    def to_a = @fields

    # `nil` for a name this layout has no field for, as the gem answers.
    def [](name) = @fields.find { |f| f.name == name }

    def offset_of(name) = self[name].offset
  end

  # A library module `extend`s this and speaks in DIRECTIVES.
  #
  # zeo has TWO tiers for them, and which one runs is decided by whether the
  # compiler could see the declaration. A declaration written in a file the
  # compiler lowered is consumed there: `attach_function` becomes an
  # `FfiCall` node whose whole C signature rides in `.rodata`, and nothing
  # below ever runs. A declaration the compiler could NOT see -- one a
  # run-time `eval` compiled, a `def self.extended(host)` hook replaying
  # `host.typedef`, a directive under a computed send -- runs here, and
  # attaches for real over the gem's own runtime objects
  # (`DynamicLibrary`, `Function`, `VariadicInvoker`).
  #
  # The two tiers answer the same questions with the same engine: both end
  # at libffi, both resolve a symbol with `dlsym`, both mangle a bare
  # library name through `LibraryPath`.
  module Library
    LIBC = FFI::Platform::LIBC

    # `typedef :uint32, :OM_uint32`. The compiler resolved every alias for
    # the signatures it lowered; recording it here as well is what lets a
    # signature attached at RUN time name one.
    def typedef(old = nil, add = nil, *)
      FFI.add_typedef(old, add) if old && add
      nil
    end

    # cdecl vs stdcall is a 32-bit Windows distinction; no target zeo
    # builds for has a second convention to pick.
    def ffi_convention(*) end

    # The libraries every subsequent `attach_function` searches, in
    # declaration order. Each name is opened EAGERLY -- an unopenable
    # library raises `LoadError` at this statement, as the gem does -- and
    # is tried as written and then through `LibraryPath`'s mangling
    # (`m` -> `libm.dylib`).
    def ffi_lib(*names)
      flags = FFI::DynamicLibrary::RTLD_LAZY | FFI::DynamicLibrary::RTLD_LOCAL
      @ffi_libs = names.flatten.map do |name|
        if name.equal?(FFI::CURRENT_PROCESS)
          FFI::DynamicLibrary.open(nil, flags)
        else
          zeo_open_library(name, flags)
        end
      end
    end

    # The gem raises rather than defaulting to the process image: an
    # `attach_function` with no library named is a declaration bug.
    def ffi_libraries
      raise LoadError, "no library specified" if @ffi_libs.nil? || @ffi_libs.empty?

      @ffi_libs
    end

    # `attach_function(name, [args], ret)` or
    # `attach_function(ruby_name, c_name, [args], ret)`, either with a
    # trailing options hash. Answers the callable it installed, as the gem
    # does, and installs it BOTH as a singleton method and as a public
    # instance method -- a library module is `include`d as often as it is
    # called through.
    def attach_function(name, a2, a3, a4 = nil, a5 = nil)
      cname, arg_types, ret_type, options =
        if a4 && (a2.is_a?(String) || a2.is_a?(Symbol))
          [a2, a3, a4, a5]
        else
          [name.to_s, a2, a3, a4]
        end
      map = options && options[:type_map]
      types = Array(arg_types).map { |t| FFI.find_type(t, map) }
      ret = FFI.find_type(ret_type, map)
      ptr = zeo_find_symbol(cname, :find_function)
      fn = if types.include?(FFI::Type::Builtin::VARARGS)
             FFI::VariadicInvoker.new(ptr, types, ret, options || {})
           else
             FFI::Function.new(ret, types, ptr, options || {})
           end
      define_singleton_method(name) { |*args, &blk| fn.call(*args, &blk) }
      define_method(name) { |*args, &blk| fn.call(*args, &blk) }
      fn
    end

    # `attach_variable(name, [c_name,] type)`: a reader and a writer over
    # the symbol's own storage. There is no compile-time tier for this one
    # -- the compiler leaves the directive as an ordinary call -- so this
    # runs for a declaration in a compiled file too.
    def attach_variable(name, a2, a3 = nil)
      cname, type = a3 ? [a2, a3] : [name.to_s, a2]
      ptr = zeo_find_symbol(cname, :find_variable)
      reader, writer = FFI::Library.zeo_accessors(FFI.find_type(type))
      define_singleton_method(name) { ptr.send(reader) }
      define_singleton_method("#{name}=") { |v| ptr.send(writer, v); v }
      ptr
    end

    # The `Pointer` read/write pair one native type is stored through.
    # `:string` is deliberately absent: a `char *` variable is a POINTER to
    # the bytes, and reading it as a String would need a second
    # dereference the gem spells `read_pointer.read_string`.
    ZEO_ACCESSORS = {
      "INT8" => %i[read_int8 write_int8], "UINT8" => %i[read_uint8 write_uint8],
      "INT16" => %i[read_int16 write_int16], "UINT16" => %i[read_uint16 write_uint16],
      "INT32" => %i[read_int32 write_int32], "UINT32" => %i[read_uint32 write_uint32],
      "INT64" => %i[read_int64 write_int64], "UINT64" => %i[read_uint64 write_uint64],
      "LONG" => %i[read_long write_long], "ULONG" => %i[read_ulong write_ulong],
      "FLOAT32" => %i[read_float write_float], "FLOAT64" => %i[read_double write_double],
      "BOOL" => %i[read_int8 write_int8], "POINTER" => %i[read_pointer write_pointer]
    }.freeze

    def self.zeo_accessors(type)
      ZEO_ACCESSORS[type.inspect] ||
        raise(TypeError, "`attach_variable` cannot store a #{type.inspect} (zeo limitation)")
    end

    private

    def zeo_open_library(name, flags)
      errors = []
      [name.to_s, FFI::LibraryPath.wrap(name.to_s).to_s].uniq.each do |candidate|
        return FFI::DynamicLibrary.open(candidate, flags)
      rescue LoadError => e
        errors << e.message
      end
      raise LoadError, "Could not open library '#{name}': #{errors.join('; ')}"
    end

    # `dlsym` across the module's libraries in declaration order, exactly
    # as the gem's own `attach_function` searches them.
    def zeo_find_symbol(cname, verb)
      libs = ffi_libraries
      libs.each do |lib|
        sym = lib.send(verb, cname.to_s)
        return sym unless sym.null?
      end
      raise FFI::NotFoundError,
            "Function '#{cname}' not found in [#{libs.map { |l| l.name || 'current process' }.join(', ')}]"
    end
  end

  # The custom-parameter-type protocol. zeo's marshaling converts through
  # `#to_ptr` rather than these hooks, so the module only records the declared
  # native type; classes extending it (fiddle's Pointer) override
  # `to_native`/`from_native` themselves.
  module DataConverter
    def native_type(type = nil)
      @native_type = type unless type.nil?
      @native_type
    end

    def to_native(value, _ctx)
      value
    end

    def from_native(value, _ctx)
      value
    end
  end
end
