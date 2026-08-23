use crate::support::run_ruby;

#[test]
fn external_gem_store_resolves_pure_ruby_and_excludes_native() {
    // `--gem-path` + `--lockfile` against a self-contained fixture
    // store (tests/fixtures/gem_store/store). Covers all three provider
    // paths: a pure-Ruby gem resolves and compiles; a gem shipping its C as
    // SOURCE is compiled from it; a precompiled-platform-only gem is
    // excluded, with a reason recorded in the disclosure report.
    let store =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
    let report = std::env::temp_dir().join(format!("zeo-store-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&report);
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        gem_report: Some(report.clone()),
        ..Default::default()
    };
    zeo::check_program_with("require \"purelib\"\nputs Purelib::VERSION\n", &opts)
        .expect("a pure-Ruby store gem resolves and compiles");
    let json = std::fs::read_to_string(&report).unwrap();
    let _ = std::fs::remove_file(&report);
    assert!(
        json.contains(r#""purelib": {"by": "bundled-gem""#),
        "{json}"
    );
    assert!(
        json.contains(r#""precompiled": {"by": null, "excluded": "precompiled-platform-gem""#),
        "{json}"
    );
}

/// A gem that ships its C as SOURCE is COMPILED, loaded and called.
///
/// The whole path in one test: the gemspec's `extensions` names an
/// `extconf.rb`, zeo runs it, mkmf writes a Makefile, zeo compiles and links
/// without `make`, and the program dlopens the result and calls a method
/// `Init_nativelib` defined. Nothing short of running it proves the last
/// three steps.
#[test]
fn a_store_gem_shipping_c_source_is_compiled_and_loaded() {
    if std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: this machine has no C compiler");
        return;
    }
    let store =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        ..Default::default()
    };
    let out = crate::support::compile_link_run(
        "require \"nativelib\"\np Nativelib::BUILT\nputs Nativelib.greet\n",
        &opts,
        &[],
        &[],
    );
    assert_eq!(out.stdout, "true\nhello from C\n", "stderr: {}", out.stderr);
}

/// The build is OUT OF TREE. A gem store is shared and often read only, so a
/// build that wrote into it would leave one project's artifacts where
/// another project reads them -- and would dirty this fixture in the repo.
#[test]
fn building_an_extension_leaves_the_gem_store_untouched() {
    if std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: this machine has no C compiler");
        return;
    }
    let store =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
    let ext = store.join("gems/nativelib-1.0.0/ext/nativelib");
    let before: Vec<String> = listing(&ext);
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        ..Default::default()
    };
    zeo::check_program_with("require \"nativelib\"\n", &opts).expect("the extension builds");
    assert_eq!(
        listing(&ext),
        before,
        "the build wrote into the gem store; it must stage into the cache"
    );
}

fn listing(dir: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The lockfile picks the version, not the store.
///
/// The fixture store holds purelib 1.0.0 AND 2.0.0 while the lockfile pins
/// 1.0.0, so a resolver that took the newest -- which is what `require` does
/// in a plain RubyGems process -- would answer 2.0.0. This compiles and RUNS
/// the program, because reaching codegen only proves a version resolved, not
/// which one ended up in the binary.
#[test]
fn the_lockfile_selects_the_version_when_the_store_holds_several() {
    let store =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gem_store/store");
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        ..Default::default()
    };
    let out = crate::support::compile_link_run(
        "require \"purelib\"\nputs Purelib::VERSION\n",
        &opts,
        &[],
        &[],
    );

    assert_eq!(
        out.stdout, "1.0.0\n",
        "the store also holds purelib 2.0.0; the lockfile pins 1.0.0"
    );
}

#[test]
fn file_read_applies_external_and_internal_encodings() {
    // File.read tags bytes with the external encoding (default UTF-8), or a
    // requested one; binread is always ASCII-8BIT; binwrite round-trips raw
    // bytes. Verified against ruby 4.0.6.
    let result = run_ruby(
        r#"
        Dir.mktmpdir do |dir|
          path = File.join(dir, "f.txt")
          File.write(path, "café")
          p File.read(path).encoding
          p File.read(path).bytesize
          p File.binread(path).encoding
          p File.binread(path).bytes
          p File.read(path, encoding: "ISO-8859-1").encoding
          p File.read(path, encoding: "ISO-8859-1").bytes
          File.binwrite(path, [0, 255, 128].pack("C*"))
          p File.binread(path).bytes
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<Encoding:UTF-8>\n5\n#<Encoding:BINARY (ASCII-8BIT)>\n\
         [99, 97, 102, 195, 169]\n#<Encoding:ISO-8859-1>\n\
         [99, 97, 102, 195, 169]\n[0, 255, 128]\n"
    );
}

// -- The fiber-backed Enumerator (per CRuby's enumerator.c).
// Every positive expectation below is oracle-verified against ruby 4.0.6.

/// The keystone: external iteration over a method-backed enumerator --
/// `next` advances a real fiber, `peek` caches without consuming, the
/// classic `loop { e.next }` idiom terminates via StopIteration and
/// returns the underlying `each`'s result, and a rescued StopIteration
/// exposes `message`/`result`.
#[test]
fn external_iteration_drives_a_real_fiber() {
    let result = run_ruby(
        r#"
        e = [10, 20, 30].each
        r = loop do
          puts e.next
        end
        p r
        e2 = [1].each
        e2.next
        begin
          e2.next
        rescue StopIteration => ex
          puts "rescued: #{ex.message}"
          p ex.result
        end
        g = [7, 8].each
        p g.peek
        p g.peek
        p g.next
        p g.next
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "10\n20\n30\n[10, 20, 30]\nrescued: iteration reached an end\n[1]\n7\n7\n7\n8\n"
    );
}

// --- A runtime-installed singleton method threads its call-site block
// through ProcData, so `yield`/`block_given?`/`&blk` work inside a
// per-object singleton (`def obj.m`, `class << obj`) and a `def` in a
// Class.new block.

#[test]
fn external_singleton_def_yields_its_call_site_block() {
    let result = run_ruby(
        r#"
        obj = Object.new
        def obj.greet
          yield "world"
        end
        puts obj.greet { |x| "hello #{x}" }
        def obj.maybe
          block_given? ? yield(5) : "none"
        end
        puts obj.maybe
        puts obj.maybe { |n| n * 100 }
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello world\nnone\n500\n");
}

/// The real `ffi` gem API, AOT-compiled: `require "ffi"` is a native
/// no-op, `extend FFI::Library` marks the module, and `attach_function` (plain
/// and the 4-arg rename form) emits a compile-time `extern "C"` + `#[link]` and
/// a wrapper module method. Scalar marshaling (`:int`/`:string`/`:ulong`/
/// `:double`) is byte-identical to CRuby+ffi -- the whole point of the
/// CRuby-faithful surface. libc/libm are always present, so this runs anywhere.
#[test]
fn ffi_attach_function_calls_c_via_extern() {
    let result = run_ruby(
        r#"
        require "ffi"
        module LibC
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          attach_function :abs, [:int], :int
          attach_function :my_strlen, :strlen, [:string], :ulong
        end
        module LibM
          extend FFI::Library
          ffi_lib "m"
          attach_function :pow, [:double, :double], :double
        end
        puts LibC.abs(-7)
        puts LibC.my_strlen("hello world")
        puts LibM.pow(2.0, 10.0)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n11\n1024.0\n");
}

/// An FFI integer argument given a non-Integer is a `TypeError`, exactly as the
/// gem raises -- the marshaling is faithful, not a silent coercion.
#[test]
fn ffi_argument_type_mismatch_raises_typeerror() {
    let result = run_ruby(
        r#"
        require "ffi"
        module L
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          attach_function :abs, [:int], :int
        end
        begin
          L.abs("not an int")
          puts "no error"
        rescue TypeError
          puts "TypeError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "TypeError\n");
}

/// FFI `typedef :existing, :alias`: a library-local type alias,
/// declared before use as the gem requires, resolves in a later
/// `attach_function`'s type list. Behaves identically to naming the underlying
/// type -- a pure compile-time aliasing, matching the gem.
#[test]
fn ffi_typedef_aliases_a_scalar_type() {
    let result = run_ruby(
        r#"
        require "ffi"
        module L
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          typedef :int, :myint
          typedef :myint, :myint2
          attach_function :abs, [:myint2], :myint
        end
        puts L.abs(-5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n");
}

/// FFI `callback :tag, [args], ret` declares a callback TYPE. A C callback is a
/// function pointer, so the tag resolves to `:pointer` and is usable anywhere a
/// pointer is -- here, a NULL passed straight back through. Passing a Ruby Proc
/// as a callback argument needs a runtime C-call builder and is still a gap.
#[test]
fn ffi_callback_declares_a_function_pointer_type() {
    let result = run_ruby(
        r#"
        require "ffi"
        module L
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          callback :cmp, [:pointer, :pointer], :int
          attach_function :abs, [:int], :int
          attach_function :my_len, :strlen, [:string], :ulong
        end
        p L.abs(-5)
        p L.my_len("hello")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n5\n");
}

/// FFI `FFI::MemoryPointer`: an owned heap buffer with typed
/// read/write accessors, pointer arithmetic, typed arrays, `from_string`, and
/// an out-of-bounds `IndexError` -- byte-identical to `ffi 1.17.4`.
#[test]
fn ffi_memory_pointer_reads_writes_and_arithmetic() {
    let result = run_ruby(
        r#"
        require "ffi"
        p = FFI::MemoryPointer.new(:int, 3)
        p.put_int(0, 10); p.put_int(4, 20); p.put_int(8, 30)
        puts p.get_int(0)
        puts p.get_int(4)
        puts (p + 8).read_int
        puts p.size
        p.write_array_of_int([7, 8, 9])
        puts p.read_array_of_int(3).inspect
        sp = FFI::MemoryPointer.from_string("hi there")
        puts sp.read_string
        puts sp.size
        d = FFI::MemoryPointer.new(:double, 1)
        d.write_double(3.5)
        puts d.read_double
        begin
          FFI::MemoryPointer.new(:int, 1).get_int(4)
          puts "no error"
        rescue IndexError
          puts "IndexError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "10\n20\n30\n12\n[7, 8, 9]\nhi there\n9\n3.5\nIndexError\n"
    );
}

/// FFI `:pointer` marshaling: a `MemoryPointer` passed to a C
/// function as its raw address, and a C `char *` return wrapped back as an
/// `FFI::Pointer`. `strcpy(buf, "hello")` fills the buffer and returns it;
/// `strlen(buf)` reads it back through the pointer. Matches CRuby+ffi.
#[test]
fn ffi_pointer_round_trips_through_c() {
    let result = run_ruby(
        r#"
        require "ffi"
        module C
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          attach_function :strcpy, [:pointer, :string], :pointer
          attach_function :strlen, [:pointer], :ulong
        end
        buf = FFI::MemoryPointer.new(:char, 32)
        ret = C.strcpy(buf, "hello")
        puts buf.read_string
        puts C.strlen(buf)
        puts ret.read_string
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello\n5\nhello\n");
}

/// FFI `enum`: a named `enum :tag, [...]` used as an
/// `attach_function` type. A Symbol argument marshals to its int; an int return
/// maps back to its Symbol (an unmapped int stays an Integer). Auto-increment
/// after an explicit value (`:next` = 101). Verified against `ffi 1.17.4` using
/// `labs` as a pass-through: `labs(:hundred)` sends 100, `labs(-100)` returns
/// 100 -> `:hundred`.
#[test]
fn ffi_enum_marshals_symbols_and_ints() {
    let result = run_ruby(
        r#"
        require "ffi"
        module C
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          enum :nums, [:zero, 0, :hundred, 100, :next]
          attach_function :to_num, :labs, [:nums], :long
          attach_function :from_num, :labs, [:long], :nums
        end
        puts C.to_num(:hundred)
        puts C.from_num(-100).inspect
        puts C.from_num(-101).inspect
        puts C.from_num(-5).inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n:hundred\n:next\n5\n");
}

/// FFI `FFI::Struct` + `layout`: a `class T < FFI::Struct`
/// with a `layout` gets synthesized `[]`/`[]=`/`size`/`offset_of`/`members`
/// over an owned `FFI::MemoryPointer`, with C field offsets/alignment. The
/// struct is auto-converted to its pointer when passed to a C `:pointer`
/// argument (`gettimeofday` fills `tv_sec`). Byte-identical to `ffi 1.17.4`.
#[test]
fn ffi_struct_layout_fields_and_c_call() {
    let result = run_ruby(
        r#"
        require "ffi"
        class Timeval < FFI::Struct
          layout :tv_sec, :long, :tv_usec, :int
        end
        puts Timeval.size
        puts Timeval.offset_of(:tv_usec)
        puts Timeval.members.inspect
        t = Timeval.new
        t[:tv_sec] = 123
        t[:tv_usec] = 456
        puts t[:tv_sec]
        puts t[:tv_usec]

        class Mixed < FFI::Struct
          layout :a, :int8, :b, :long, :c, :int
        end
        puts Mixed.size
        puts Mixed.offset_of(:b)
        puts Mixed.offset_of(:c)

        module C
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          attach_function :gettimeofday, [:pointer, :pointer], :int
        end
        tv = Timeval.new
        C.gettimeofday(tv, nil)
        puts(tv[:tv_sec] > 1_000_000)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "16\n8\n[:tv_sec, :tv_usec]\n123\n456\n24\n8\n16\ntrue\n"
    );
}

// A variadic `attach_function [.., :varargs]` builds its call interface at
// runtime through libffi: `snprintf` formats mixed int/string/double varargs
// into a buffer (and a call with no varargs at all still works). Oracle-pinned
// against ruby 4.0.6 + the real `ffi` gem.
#[test]
fn variadic_attach_function_matches_the_oracle() {
    let result = run_ruby(
        r#"
        require "ffi"
        module C
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          attach_function :snprintf, [:pointer, :size_t, :string, :varargs], :int
        end
        buf = FFI::MemoryPointer.new(:char, 64)
        n = C.snprintf(buf, 64, "%d/%s/%.1f", :int, 42, :string, "hi", :double, 2.5)
        puts n
        puts buf.read_string
        buf2 = FFI::MemoryPointer.new(:char, 16)
        C.snprintf(buf2, 16, "plain")
        puts buf2.read_string
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "9\n42/hi/2.5\nplain\n");
}

// A `callback` type + a Ruby Proc passed for that argument becomes a libffi
// closure the C side calls back into: `qsort` and `bsearch` are driven by Ruby
// comparators, ascending and descending.
#[test]
fn callback_proc_drives_qsort_and_bsearch() {
    let result = run_ruby(
        r#"
        require "ffi"
        module L
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          callback :cmp, [:pointer, :pointer], :int
          attach_function :qsort,   [:pointer, :size_t, :size_t, :cmp], :void
          attach_function :bsearch, [:pointer, :pointer, :size_t, :size_t, :cmp], :pointer
        end
        cmp = proc { |a, b| a.read_int64 <=> b.read_int64 }
        arr = FFI::MemoryPointer.new(:int64, 5)
        arr.write_array_of_int64([9, 3, 7, 1, 5])
        L.qsort(arr, 5, 8, cmp)
        p arr.read_array_of_int64(5)
        arr2 = FFI::MemoryPointer.new(:int64, 4)
        arr2.write_array_of_int64([10, 20, 30, 40])
        L.qsort(arr2, 4, 8, proc { |a, b| b.read_int64 <=> a.read_int64 })
        p arr2.read_array_of_int64(4)
        key = FFI::MemoryPointer.new(:int64, 1)
        key.write_int64(7)
        hit = L.bsearch(key, arr, 5, 8, cmp)
        puts(hit == nil ? "miss" : hit.read_int64)
        key.write_int64(4)
        puts(L.bsearch(key, arr, 5, 8, cmp) == nil ? "miss" : "hit")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 3, 5, 7, 9]\n[40, 30, 20, 10]\n7\nmiss\n"
    );
}

// An exception raised inside a callback cannot unwind through the C frames, so
// it is stashed and re-raised once the C function returns.
#[test]
fn exception_in_callback_is_reraised_after_the_c_call() {
    let result = run_ruby(
        r#"
        require "ffi"
        module L
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          callback :cmp, [:pointer, :pointer], :int
          attach_function :qsort, [:pointer, :size_t, :size_t, :cmp], :void
        end
        arr = FFI::MemoryPointer.new(:int32, 3)
        arr.write_array_of_int32([3, 1, 2])
        begin
          L.qsort(arr, 3, 4, proc { |a, b| raise "boom from callback" })
        rescue => e
          puts "rescued: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: boom from callback\n");
}

/// The declaration spellings the corpus writes that the strict forms missed:
/// a String function name (the gem calls `.to_sym` on it), and
/// `ffi_lib FFI::CURRENT_PROCESS` (symbols from the already-linked image --
/// what a `lib` of `None` emits).
#[test]
fn ffi_string_names_and_current_process() {
    let result = run_ruby(
        r#"
        require "ffi"
        module L
          extend FFI::Library
          ffi_lib FFI::CURRENT_PROCESS
          attach_function "abs", [:int], :int
        end
        puts L.abs(-9)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "9\n");
}

/// `layout` written as one hash (the gem's documented alternative), a
/// root-anchored `::FFI::Struct` superclass, and an inline-array count
/// computed from `FFI::Type::X.size` -- three corpus spellings in one layout.
#[test]
fn ffi_struct_hash_layout_and_type_size_count() {
    let result = run_ruby(
        r#"
        require "ffi"
        class Pt < ::FFI::Struct
          layout x: :int32, y: :int32, pad: [:uint8, 16 / ::FFI::Type::LONG.size]
        end
        p = Pt.new
        p[:x] = 7
        p[:y] = 35
        puts p[:x] + p[:y]
        puts Pt.size
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    // 4 + 4 + 2 bytes, rounded up to the 4-byte alignment.
    assert_eq!(result.stdout, "42\n12\n");
}

/// A NAMELESS `enum [...]` registers no type name and is consumed; a class
/// that `extend FFI::DataConverter` with a `native_type` stands for that
/// native type in later declarations (google-protobuf's Internal::Arena).
#[test]
fn ffi_nameless_enum_and_data_converter() {
    let result = run_ruby(
        r#"
        require "ffi"
        module Internal
          class Arena
            extend ::FFI::DataConverter
            native_type ::FFI::Type::POINTER
          end
        end
        module L
          extend FFI::Library
          ffi_lib FFI::CURRENT_PROCESS
          enum [:small, :medium, :large]
          attach_function :my_memchr, :memchr, [Internal::Arena, :int, :size_t], :pointer
        end
        puts "compiled"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "compiled\n");
}

/// An `ffi_lib` whose name only a RUNNING process can resolve -- a path built
/// by interpolation -- takes the dlopen/dlsym tier: the first candidate that
/// opens wins, exactly as the gem tries its alternatives. The list spans both
/// platforms' libm spellings; the leading candidate never exists.
#[test]
fn ffi_runtime_dlopen_resolves_a_path_candidate_list() {
    let result = run_ruby(
        r##"
        require "ffi"
        module M
          extend FFI::Library
          ffi_lib ["#{'/no'}/such/dir/libnothing.so", "/usr/lib/libm.dylib", "libm.so.6"]
          attach_function :pow, [:double, :double], :double
        end
        puts M.pow(2.0, 8.0)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "256.0\n");
}

/// Every candidate failing to open is the gem's own `LoadError`, raised at
/// the CALL -- a binary whose optional native half is absent still starts.
#[test]
fn ffi_runtime_dlopen_failure_is_a_loaderror_at_the_call() {
    let result = run_ruby(
        r##"
        require "ffi"
        module M
          extend FFI::Library
          ffi_lib "#{'/no'}/such/dir/libnothing.so"
          attach_function :nope, [], :int
        end
        puts "started"
        begin
          M.nope
        rescue LoadError => e
          puts e.message.start_with?("Could not open library")
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "started\ntrue\n");
}

/// `.by_value` is the gem's BY-VALUE spelling: `inet_ntoa` takes `struct
/// in_addr` by value on every libc, and the repr(C) mirror carries the real
/// field types so rustc owns the ABI.
///
/// A BARE `FFI::Struct` subclass in the same position is the gem's
/// `StructByReference` -- it passes a POINTER (oracle-checked: ruby-ffi hands
/// `inet_ntoa` the struct's address and gets an address-shaped dotted quad
/// back). `gettimeofday` is the honest way to show it, since a C function that
/// really wants the pointer then works.
#[test]
fn ffi_struct_by_value_argument() {
    let result = run_ruby(
        r#"
        require "ffi"
        class InAddr < FFI::Struct
          layout s_addr: :uint32
        end
        class Timeval < FFI::Struct
          layout :tv_sec, :long, :tv_usec, :long
        end
        module L
          extend FFI::Library
          ffi_lib FFI::Library::LIBC
          attach_function :inet_ntoa, [InAddr.by_value], :string
          attach_function :gettimeofday, [Timeval, :pointer], :int
        end
        a = InAddr.new
        a[:s_addr] = 16777343
        puts L.inet_ntoa(a)
        tv = Timeval.new
        puts L.gettimeofday(tv, nil)
        puts tv[:tv_sec] > 1_600_000_000
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "127.0.0.1\n0\ntrue\n");
}
