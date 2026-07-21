use crate::support::{
    compile_packages, compile_project, run_ruby, run_ruby_packages, run_ruby_project,
};

#[test]
fn ivars_store_real_per_instance_state() {
    let result = run_ruby(
        r#"
        class Point
          def initialize(x)
            @x = x
          end

          def x
            @x
          end
        end

        puts Point.new(5).x
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n");
}

#[test]
fn if_elsif_else_as_statement_and_as_value() {
    let result = run_ruby(
        r#"
        n = -5
        if n < 0
          puts :negative
        elsif n == 0
          puts :zero
        else
          puts :positive
        end

        n2 = 0
        result = if n2 < 0
          :negative
        elsif n2 == 0
          :zero
        else
          :positive
        end
        puts result
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "negative\nzero\n");
}

#[test]
fn backtick_captures_stdout_and_sets_child_status() {
    // `` `cmd` `` lowers to a `Kernel#\`` fcall: it captures the child's
    // stdout as a String and leaves the wait status in `$?`.
    let result = run_ruby(
        r#"
        out = `echo hello`
        print out
        puts out.length
        puts $?.exitstatus
        puts $?.success?
        puts $?.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hello\n6\n0\ntrue\nProcess::Status\n");
}

#[test]
fn system_returns_true_false_and_sets_status() {
    // `system` inherits stdio and answers true (exit 0) / false (nonzero),
    // setting `$?` either way. A nonzero exit does not raise.
    let result = run_ruby(
        r#"
        p system("true")
        puts $?.exitstatus
        p system("false")
        puts $?.success?
        puts $?.exitstatus
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n0\nfalse\nfalse\n1\n");
}

#[test]
fn numbered_params_work_through_the_times_inline_path() {
    let result = run_ruby("3.times { puts _1 * 10 }");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n10\n20\n");
}

#[test]
fn multiple_modules_in_one_include_statement_resolve_in_given_order() {
    let result = run_ruby(
        r#"
        module A
          def a
            "a"
          end
        end
        module B
          def b
            "b"
          end
        end
        class C
          include A, B
        end
        c = C.new
        puts c.a
        puts c.b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\nb\n");
}

// Phase 8: `case/in` pattern matching. Every test below was oracle-verified
// against real `ruby` first, per this project's established convention.

#[test]
fn case_in_class_check_pattern_binds_and_statically_narrows_to_int() {
    // `Integer => n` both binds `n` AND statically narrows its type for the
    // rest of the arm's body -- `n + 1` should take the native `Int`
    // arithmetic fast path, not a runtime Poly fallback (verified indirectly:
    // if narrowing were broken this would still print `6`, but a fast-path
    // regression would show up as a `zeo` panic on `+` instead, since a
    // Poly local has no runtime `+` fallback for a non-builtin-typed operand
    // -- see `codegen::call::dispatch`'s docs).
    let result = run_ruby("case 5\nin Integer => n\n  puts n + 1\nend\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn else_clause_runs_only_on_the_no_exception_path() {
    let result = run_ruby(
        r#"
        begin
          puts "body"
        rescue
          puts "rescued"
        else
          puts "else ran"
        ensure
          puts "ensure ran"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "body\nelse ran\nensure ran\n");
}

#[test]
fn uncaptured_inlined_times_param_keeps_the_plain_fast_path() {
    // Regression guard for H1: when NOTHING captures the `.times` param it
    // stays a plain per-iteration `let` (no cell), and `break`/`next` inside
    // the inlined block keep working via literal labels.
    let result = run_ruby(
        r#"
        total = 0
        5.times { |i| total += i }
        puts total
        r = 5.times { |i| break i * 2 if i == 3 }
        p r
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n6\n");
}

#[test]
fn sequential_top_level_begin_blocks_do_not_leak_handling_state() {
    // Guards `zeo_rt::handling`'s push/pop discipline: three INDEPENDENT
    // `begin` blocks in sequence, the last a bare re-raise with nothing
    // currently being handled -- if an earlier block's `pop_handling` were
    // ever skipped (e.g. on an unusual exit path), this would incorrectly
    // re-raise a STALE exception instead of falling back to a fresh
    // `RuntimeError`.
    let result = run_ruby(
        r#"
        begin
          raise "first"
        rescue => e
          puts "1: #{e.send(:message)}"
        end
        begin
          raise "second"
        rescue => e
          puts "2: #{e.send(:message)}"
        end
        begin
          raise
        rescue => e
          puts "3: [#{e.send(:message)}]"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1: first\n2: second\n3: []\n");
}

#[test]
fn respond_to_true_and_false_on_a_statically_known_receiver() {
    let result = run_ruby(
        r#"
        class Dog
          def bark
            "woof"
          end
        end

        d = Dog.new
        puts d.respond_to?(:bark)
        puts d.respond_to?(:meow)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n");
}

#[test]
fn a_class_body_ivar_write_initializes_class_level_state() {
    // A bare `@x = ...` directly in a class body -- the ordinary way
    // class-level state gets seeded. It used to fall through `register_class`'s
    // catch-all arm and be SILENTLY DROPPED, leaving the reader a bare nil
    // with no diagnostic at all.
    let result = run_ruby(
        r#"
        class Registry
          @items = []
          @count = 0
          def self.add(x); @items << x; @count += 1; self; end
          def self.items; @items; end
          def self.count; @count; end
        end
        Registry.add("a").add("b")
        p Registry.items
        p Registry.count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[\"a\", \"b\"]\n2\n");
}

#[test]
fn module_level_state_accumulates_across_class_method_calls() {
    let result = run_ruby(
        r#"
        module Counter
          @n = 0
          def self.bump; @n += 1; end
          def self.n; @n; end
        end
        Counter.bump
        Counter.bump
        Counter.bump
        p Counter.n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn ruby_version_build_constants_and_file_separators() {
    // Stable values plus a self-consistency check that RUBY_DESCRIPTION is
    // composed from its parts exactly as CRuby's version.c does (portable
    // across build hosts; RUBY_PLATFORM is build-target-derived via build.rs).
    let result = run_ruby(
        r#"
        puts RUBY_VERSION
        puts RUBY_ENGINE
        puts RUBY_PATCHLEVEL
        puts File::SEPARATOR
        p File::ALT_SEPARATOR
        short = RUBY_REVISION[0, 10]
        composed = "ruby #{RUBY_VERSION} (#{RUBY_RELEASE_DATE} revision #{short}) +PRISM [#{RUBY_PLATFORM}]"
        puts RUBY_DESCRIPTION == composed
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4.0.5\nruby\n0\n/\nnil\ntrue\n");
}

#[test]
fn file_ftype_foreach_and_dir_foreach() {
    let result = run_ruby(
        r#"
        Dir.mktmpdir do |dir|
          File.write(File.join(dir, "a.txt"), "one\ntwo\nthree\n")
          Dir.mkdir(File.join(dir, "sub"))
          p File.ftype(File.join(dir, "a.txt"))
          p File.ftype(File.join(dir, "sub"))
          File.foreach(File.join(dir, "a.txt")) { |line| print "L:", line }
          p File.foreach(File.join(dir, "a.txt"), chomp: true).to_a
          # Enumerator form gives the entries; block form returns nil.
          p Dir.foreach(dir).to_a.sort
          p(Dir.foreach(dir) { |e| })
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"file\"\n\"directory\"\nL:one\nL:two\nL:three\n[\"one\", \"two\", \"three\"]\n\
         [\".\", \"..\", \"a.txt\", \"sub\"]\nnil\n"
    );
}

#[test]
fn is_a_and_kind_of_against_a_statically_known_int_local() {
    let result = run_ruby(
        r#"
        x = 5
        puts x.is_a?(Integer)
        puts x.is_a?(String)
        puts x.is_a?(Object)
        puts x.kind_of?(Integer)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\ntrue\ntrue\n");
}

#[test]
fn is_a_against_statically_typed_array_hash_range_and_symbol_locals() {
    let result = run_ruby(
        r#"
        arr = [1, 2, 3]
        puts arr.is_a?(Array)
        puts arr.is_a?(Object)
        puts arr.is_a?(Hash)
        h = {a: 1}
        puts h.is_a?(Hash)
        r = 1..5
        puts r.is_a?(Range)
        sym = :foo
        puts sym.is_a?(Symbol)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\nfalse\ntrue\ntrue\ntrue\n");
}

#[test]
fn file_line_and_dir_name_the_file_the_code_was_written_in() {
    // The point of the SOURCE_FILE stack: `require` merges every file's
    // statements into one Program, so by codegen time nothing tells them
    // apart -- these have to be resolved at LOWERING time, per file. A
    // `__FILE__` inside a required file must name THAT file, not the main
    // one, and the main file's own `__FILE__` after the require must be
    // itself again.
    //
    // Only basenames are compared: the harness compiles from a per-test
    // temp dir, so the absolute paths differ per run.
    let result = run_ruby_project(
        &[
            (
                "helper.rb",
                "def helper_file; __FILE__; end\ndef helper_line; __LINE__; end\ndef helper_dir; __dir__; end\n",
            ),
            (
                "main.rb",
                r#"
                require_relative "helper"
                puts File.basename(helper_file)
                puts helper_line
                puts File.basename(__FILE__)
                puts __LINE__
                puts helper_dir == __dir__
                puts __dir__.start_with?("/")
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    // helper_line is 2 (the `def helper_line` line); the main file's
    // `__LINE__` is on its own 6th line counting the leading newline.
    //
    // `__dir__` is only checked for absoluteness and for agreeing between
    // the two files, NOT compared against `File.expand_path(__FILE__)`:
    // `__dir__` is baked at COMPILE time while `expand_path` of a relative
    // `__FILE__` resolves against the RUNTIME cwd, and this harness runs
    // the binary from a different directory than it compiled in. The two
    // agree whenever the program is run from its own directory, which is
    // verified against the oracle separately.
    assert_eq!(result.stdout, "helper.rb\n2\nmain.rb\n6\ntrue\ntrue\n");
}

#[test]
fn pack_and_unpack_roundtrip_core_directives() {
    // Array#pack / String#unpack across the integer, string, base64, hex,
    // BER and UTF-8 directives. Verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        p [65, 66, 67].pack("C*")
        p [258].pack("v").bytes
        p [258].pack("S>").bytes
        p [-1].pack("l").bytes
        p "\x00\x00\x00\x01".unpack("N")
        p "\xff\xff\xff\xff".unpack("l")
        p "\xff\xff\xff\xff".unpack("L")
        p ["hi"].pack("a5").bytes
        p ["hi"].pack("A5").bytes
        p "abc\0de".unpack("Z*")
        p ["hello world"].pack("m")
        p "aGVsbG8=\n".unpack("m")
        p ["ff01"].pack("H*").bytes
        p [300].pack("w").bytes
        p [12354].pack("U")
        p "あ".unpack("U*")
        p [65].pack("C").encoding
        p [12354].pack("U").encoding
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ABC\"\n[2, 1]\n[1, 2]\n[255, 255, 255, 255]\n[1]\n[-1]\n[4294967295]\n\
         [104, 105, 0, 0, 0]\n[104, 105, 32, 32, 32]\n[\"abc\"]\n\
         \"aGVsbG8gd29ybGQ=\\n\"\n[\"hello\"]\n[255, 1]\n[130, 44]\n\
         \"\u{3042}\"\n[12354]\n\
         #<Encoding:BINARY (ASCII-8BIT)>\n#<Encoding:UTF-8>\n"
    );
}

#[test]
fn a_global_alias_shares_storage_in_both_directions() {
    // `alias $copy $orig` is a real alias, not a copy: one slot, two names,
    // and writing EITHER is visible through the other. Oracle-verified both
    // ways -- which is why it can't lower to `$copy = $orig`.
    let result = run_ruby(
        r#"
        $orig = 5
        alias $copy $orig
        $copy = 7
        p $orig
        p $copy
        $orig = 9
        p [$orig, $copy]
        alias $b $never_set
        p $b
        $never_set = 1
        p $b
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n7\n[9, 9]\nnil\n1\n");
}

#[test]
fn class_shift_self_containing_an_unsupported_statement_is_a_clean_lowering_error() {
    // `def`s, constants, `include`, and `attr_*`/`private`/`alias` are handled
    // in `class << self`; an ivar assignment on the singleton (`@x = 1`) is not
    // -- a clean rejection, not silently ignored.
    let err = zeo::compile_to_rust(
        r#"
        class Foo
          class << self
            @x = 1
          end
        end
        "#,
    )
    .unwrap_err();
    assert!(
        err.contains("unsupported statement in `class << self`"),
        "{err}"
    );
}

#[test]
fn freeze_returns_self_keeping_the_static_collection_type() {
    // Exercises `types.rs`'s `.freeze`-returns-self inference: `names[0]`/
    // `names.length` must still take the static Array fast path (a Poly
    // fallback would panic). A LOCAL, not the classic `NAMES = [...].freeze`
    // constant idiom: a constant READ is always Poly (constants live in a
    // runtime map with no static type tracking) -- a PRE-existing gap that
    // makes `A = [1]; A[0]` fail with or without `.freeze` involved, noted
    // for a later phase, not a freeze regression.
    let result = run_ruby(
        r#"
        names = ["a", "b"].freeze
        puts names[0]
        puts names.length
        puts names.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "a\n2\ntrue\n");
}

#[test]
fn kernel_caller_and_conversion_functions_resolve_through_every_dispatch_path() {
    // `caller`/`caller_locations` answer an empty Array (no runtime frames in an
    // AOT build), and the private Kernel conversion/format helpers resolve as
    // real methods -- so a splat call, `method(:Integer)`, or a forwarded block
    // reach them, not only the codegen fast-path. Byte-verified against ruby 4.0.5.
    let result = run_ruby(
        r#"
        def frames; caller; end
        p frames.is_a?(Array)
        p caller_locations(1, 1).is_a?(Array)
        args = ["%d-%s", 3, "x"]
        puts format(*args)
        p [1, 2, 3].map(&method(:Integer))
        p ["1", "0xff"].map(&method(:Integer))
        def wrap(&b); proc(&b); end
        p wrap { |x| x + 100 }.call(1)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n3-x\n[1, 2, 3]\n[1, 255]\n101\n");
}

#[test]
fn file_predicates_times_and_fnm_constants() {
    // File-type/permission predicates, time accessors, and the FNM_* flag
    // constants, exercised on a file the test writes.
    let result = run_ruby(
        r#"
        p [File::FNM_DOTMATCH, File::FNM_PATHNAME, File::FNM_CASEFOLD, File::FNM_NOESCAPE, File::FNM_EXTGLOB]
        path = "/tmp/sp_e2e_file_probe"
        File.write(path, "hi"); File.chmod(0644, path)
        p File.world_readable?(path)
        p [File.pipe?(path), File.socket?(path), File.chardev?(path), File.blockdev?(path)]
        p [File.owned?(path), File.setuid?(path), File.sticky?(path)]
        p File.identical?(path, path)
        p [File.birthtime(path).class, File.atime(path).class, File.ctime(path).class]
        link = "/tmp/sp_e2e_file_link"
        File.symlink(path, link)
        p [File.symlink?(link), File.readlink(link) == path]
        File.delete(link); File.delete(path)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[4, 2, 8, 1, 16]\n420\n[false, false, false, false]\n[true, false, false]\ntrue\n\
         [Time, Time, Time]\n[true, true]\n"
    );
}

#[test]
fn file_stat_io_pipe_and_io_read_family() {
    // Batch 10: File::Stat (class + readers), File.stat/File#stat, IO.pipe
    // (a plain-IO reader/writer pair), the IO instance read family
    // (gets separator/limit/chomp, getc/getbyte, lineno), IO class methods
    // (IO.read/write/copy_stream), and FileTest.
    let result = run_ruby(
        r#"
        path = "/tmp/sp_e2e_stat_#{Process.pid}.txt"
        File.write(path, "hello\nworld\n")
        st = File.stat(path)
        p st.class
        p st.size
        p st.file?
        p st.directory?
        File.open(path) { |f| p f.stat.class }
        File.open(path) { |f| p f.gets("o") }
        File.open(path) { |f| p f.gets(3) }
        File.open(path) { |f| p f.gets(chomp: true); p f.lineno }
        File.open(path) { |f| p f.getc; p f.getbyte }
        p FileTest.file?(path)
        p FileTest.directory?(path)
        r, w = IO.pipe
        p r.class
        w.write("ping")
        w.close
        p r.read
        r.close
        p IO.read(path, 5)
        File.delete(path)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "File::Stat\n12\ntrue\nfalse\nFile::Stat\n\"hello\"\n\"hel\"\n\"hello\"\n1\n\
         \"h\"\n101\ntrue\nfalse\nIO\n\"ping\"\n\"hello\"\n"
    );
}

#[test]
fn dir_handle_argf_class_and_binding_local_variable_get() {
    // Batch 11: Dir.new/Dir.open handles (#path/#read/#each/#children/#entries/
    // #rewind/#close, the block form, ENOENT on a missing path), ARGF's
    // literally-named class and default "-" filename with no file args, and
    // binding.local_variable_get(:name) reading an in-scope local (including a
    // reserved-word parameter).
    let result = run_ruby(
        r##"
        dir = "/tmp/sp_e2e_dirh_#{Process.pid}"
        Dir.mkdir(dir) unless Dir.exist?(dir)
        File.write("#{dir}/x", "")
        File.write("#{dir}/y", "")
        d = Dir.new(dir)
        p d.class
        p d.path == dir
        names = []
        d.each { |n| names << n }
        p names.sort
        d.rewind
        p d.read.class
        d.close
        r = Dir.open(dir) { |dd| dd.children.sort }
        p r
        p((Dir.new("/nonexistent_zz_e2e") rescue $!.class))
        File.delete("#{dir}/x", "#{dir}/y")
        Dir.rmdir(dir)

        p ARGF.class
        p ARGF.filename

        def read_reserved(then:)
          binding.local_variable_get(:then)
        end
        p read_reserved(then: :tick)
        x = [1, 2, 3]
        p binding.local_variable_get(:x)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Dir\ntrue\n[\".\", \"..\", \"x\", \"y\"]\nString\n[\"x\", \"y\"]\n\
         Errno::ENOENT\nARGF.class\n\"-\"\n:tick\n[1, 2, 3]\n"
    );
}

#[test]
fn diamond_requires_execute_the_shared_file_exactly_once() {
    let result = run_ruby_project(
        &[
            ("shared.rb", "puts \"shared executed\"\nSHARED = 7\n"),
            (
                "liba.rb",
                "require_relative \"shared\"\nputs \"liba loaded\"\n",
            ),
            (
                "libb.rb",
                "require_relative \"shared\"\nputs \"libb loaded\"\n",
            ),
            (
                "main.rb",
                r#"
                    require_relative "liba"
                    require_relative "libb"
                    require_relative "shared"
                    puts SHARED
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "shared executed\nliba loaded\nlibb loaded\n7\n"
    );
}

#[test]
fn an_earlier_i_root_shadows_a_later_same_named_stdlib_file() {
    // The documented precedence: `-I` roots are searched in order, first hit
    // wins (Ruby's own `$LOAD_PATH` rule) -- so a user root placed BEFORE the
    // stdlib checkout shadows the stdlib copy of a same-named feature, and the
    // later root's file is never loaded.
    let result = run_ruby_project(
        &[
            (
                "userlib/patched.rb",
                r#"
                    module Patched
                      def self.source = "user override"
                    end
                "#,
            ),
            (
                "stdliblib/patched.rb",
                r#"
                    module Patched
                      def self.source = "stdlib original"
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    require "patched"
                    puts Patched.source
                "#,
            ),
        ],
        "main.rb",
        &["userlib", "stdliblib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "user override\n");
}

#[test]
fn autoload_nested_in_a_module_eagerly_splices_the_feature_file() {
    // #101: `autoload :Const, "feature"` is treated as a compile-time
    // require -- the loader's eager pre-pass splices the feature file (at any
    // structural nesting) so the constant is defined; the `autoload` call
    // itself is a no-op. Documented divergence from CRuby's laziness (loads at
    // the autoload site, not first access), but observationally identical for
    // a definitional autoloaded file.
    let result = run_ruby_project(
        &[
            (
                "lib/greeter.rb",
                r##"
                    module App
                      module Greeter
                        def self.hi(n); "hi #{n}"; end
                      end
                    end
                "##,
            ),
            (
                "main.rb",
                r#"
                    module App
                      autoload :Greeter, "greeter"
                    end
                    puts App::Greeter.hi("bob")
                "#,
            ),
        ],
        "main.rb",
        &["lib"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi bob\n");
}

#[test]
fn autoload_with_file_expand_path_dir_resolves_a_sibling_file() {
    // The dominant stdlib/bundler idiom: `autoload :C, File.expand_path("c",
    // __dir__)` -- a computed sibling path. Resolved at compile time from the
    // requiring file's directory.
    let result = run_ruby_project(
        &[
            (
                "mirror.rb",
                r#"
                    class Config
                      class Mirror
                        def self.name; "mirror!"; end
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    class Config
                      autoload :Mirror, File.expand_path("mirror", __dir__)
                    end
                    puts Config::Mirror.name
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "mirror!\n");
}

#[test]
fn a_required_files_top_level_locals_are_isolated_from_the_main_file() {
    // Real Ruby gives every file its own top-level local scope: main's
    // `count` and the lib's `count` (mutated through a block, exercising
    // the rename pass inside shared-scope block bodies) never touch. The
    // lib hands its result out through a global -- the only channel real
    // Ruby shares.
    let result = run_ruby_project(
        &[
            (
                "counterlib.rb",
                r#"
                    count = 100
                    3.times do
                      count += 1
                    end
                    $lib_count = count
                    class CounterBox
                      def initialize
                        @n = 0
                      end
                      def bump
                        @n += 1
                      end
                      def n
                        @n
                      end
                    end
                "#,
            ),
            (
                "main.rb",
                r#"
                    count = 5
                    require_relative "counterlib"
                    puts count
                    puts $lib_count
                    b = CounterBox.new
                    b.bump
                    b.bump
                    puts b.n
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\n103\n2\n");
}

#[test]
fn missing_require_relative_reports_the_absolutized_path() {
    let err = compile_project(
        &[("main.rb", "require_relative \"nope\"\n")],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(
        // CRuby names the absolutized path WITHOUT the extension it tried:
        // `require_relative "nope"` from /tmp says `... -- /tmp/nope`.
        err.contains("cannot load such file -- ")
            && err.contains("nope")
            && !err.contains("nope.rb"),
        "unexpected error: {err}"
    );
}

#[test]
fn pathless_require_relative_cannot_infer_basepath() {
    // `compile_to_rust` (no input path) mirrors CRuby's eval/irb context:
    // require_relative has no requiring-file directory to resolve against.
    let err = zeo::compile_to_rust("require_relative \"x\"\n").unwrap_err();
    assert!(
        err.contains("cannot infer basepath"),
        "unexpected error: {err}"
    );
}

#[test]
fn a_gem_can_override_require_paths_to_a_flat_layout() {
    let result = run_ruby_packages(
        &[
            (
                "packages/flat/flat.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"flat\"\n  s.version = \"1.0.0\"\n  s.require_paths = [\".\"]\nend\n",
            ),
            ("packages/flat/flat.rb", "FLAT = \"flat pkg\"\n"),
            ("main.rb", "require \"flat\"\nputs FLAT\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "flat pkg\n");
}

#[test]
fn dash_i_roots_shadow_packages_and_earlier_package_dirs_shadow_later_ones() {
    // CRuby's own ordering: -I beats even default gems; and one package
    // NAME resolves to exactly one package, nearest package-dir first.
    let result = run_ruby_packages(
        &[
            ("override/dual.rb", "puts \"from -I root\"\n"),
            (
                "projpkgs/thing/thing.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"thing\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "projpkgs/thing/lib/thing.rb",
                "puts \"thing from projpkgs\"\n",
            ),
            (
                "bundledpkgs/thing/thing.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"thing\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "bundledpkgs/thing/lib/thing.rb",
                "puts \"thing from bundledpkgs\"\n",
            ),
            (
                "bundledpkgs/dual/dual.gemspec",
                "Gem::Specification.new do |s|\n  s.name = \"dual\"\n  s.version = \"1.0.0\"\nend\n",
            ),
            (
                "bundledpkgs/dual/lib/dual.rb",
                "puts \"dual from package\"\n",
            ),
            ("main.rb", "require \"dual\"\nrequire \"thing\"\n"),
        ],
        "main.rb",
        &["override"],
        &["projpkgs", "bundledpkgs"],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from -I root\nthing from projpkgs\n");
}

#[test]
fn a_directory_without_a_manifest_is_not_a_gem() {
    // No `.gemspec` -> ignored entirely; the feature is simply not found.
    let err = compile_packages(
        &[
            ("packages/plain/lib/plain.rb", "puts 1\n"),
            ("main.rb", "require \"plain\"\n"),
        ],
        "main.rb",
        &[],
        &["packages"],
    )
    .unwrap_err();
    assert!(
        err.contains("cannot load such file -- plain"),
        "unexpected error: {err}"
    );
}

// ---- Wave-1 in-tree extensions (stringio/strscan/cgi/digest) + json/yaml/zlib ----

#[test]
fn stringio_reads_writes_and_tracks_position() {
    let result = run_ruby(
        r#"
        require "stringio"
        io = StringIO.new
        io.puts "hello"
        io.print "world"
        puts io.string.inspect
        io.rewind
        puts io.gets.inspect
        puts io.read.inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"hello\\nworld\"\n\"hello\\n\"\n\"world\"\n"
    );
}

#[test]
fn missing_qualified_constant_raises_name_error_with_the_full_path() {
    let result = run_ruby(
        r#"
        module Store
        end

        begin
          puts Store::MISSING
        rescue NameError => e
          puts "NameError: #{e.message}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "NameError: uninitialized constant Store::MISSING\n"
    );
}

#[test]
fn qualified_class_paths_work_in_patterns_and_is_a() {
    let result = run_ruby(
        r#"
        module Store
          class Item
            def initialize(n)
              @n = n
            end

            def deconstruct_keys(keys)
              { n: @n }
            end
          end
        end

        case Store::Item.new(5)
        in Store::Item
          puts "matched class pattern"
        end

        case Store::Item.new(7)
        in Store::Item(n:)
          puts "matched with capture #{n}"
        end

        puts Store::Item.new(1).is_a?(Store::Item)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "matched class pattern\nmatched with capture 7\ntrue\n"
    );
}

/// Per-box re-execution (the same file `box.require`d into two boxes runs
/// twice, with independent class-variable state per box), and per-box
/// builtin MONKEYPATCHES: a box's `String#blank?` resolves from that box's
/// code while main's `"foo".blank?` stays a NoMethodError -- the docs'
/// motivating example, dispatch by DEFINING box.
#[test]
fn boxes_reexecute_files_and_patch_builtins_privately() {
    let result = run_ruby_project(
        &[
            (
                "blank.rb",
                "class String\n  def blank?\n    strip.empty?\n  end\nend\n\
                 class Foo\n  def self.blank_one?\n    \"   \".blank?\n  end\nend\n\
                 class Counter\n  @@count = 0\n  def self.bump\n    @@count += 1\n  end\n  def self.count\n    @@count\n  end\nend\n\
                 puts \"loaded\"\n",
            ),
            (
                "main.rb",
                "box = Ruby::Box.new\n\
                 box.require_relative \"blank\"\n\
                 box2 = Ruby::Box.new\n\
                 box2.require_relative \"blank\"\n\
                 p box::Foo.blank_one?\n\
                 begin\n  \"foo\".blank?\nrescue NoMethodError => e\n  puts \"main: #{e.message}\"\nend\n\
                 box::Counter.bump\n\
                 box::Counter.bump\n\
                 box2::Counter.bump\n\
                 p box::Counter.count\n\
                 p box2::Counter.count\n",
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "loaded\nloaded\ntrue\nmain: undefined method 'blank?' for an instance of String\n2\n1\n"
    );
}

#[test]
fn a_parenthesized_multi_statement_expression_answers_its_last_statement() {
    // `(a; b)` introduces NO scope: `a` below is still readable afterwards,
    // which is why this lowers to a plain `Seq` rather than anything that
    // pushes a scope.
    let result = run_ruby(
        r#"
        x = (1; 2; 3)
        p x
        y = (a = 5; a * 2)
        p y
        p a
        p((puts "side"; 42))
        p [(1; 2), 3]
        z = (
          q = 7
          q + 1
        )
        p z
        p((1; (2; 3)))
        w = (if true then "yes" else "no" end; "after")
        p w
        p (5)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\n10\n5\nside\n42\n[2, 3]\n8\n3\n\"after\"\n5\n"
    );
}

#[test]
fn interpolation_takes_multi_statement_empty_and_braceless_forms() {
    let result = run_ruby(
        r##"
        p "v=#{1; 2}"
        p "v=#{a = 3; a * 2}"
        p a
        p "x#{}y"
        $g = "glob"
        class C
          def initialize; @iv = "ivar"; end
          def show; "iv=#@iv g=#$g"; end
        end
        p C.new.show
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"v=2\"\n\"v=6\"\n3\n\"xy\"\n\"iv=ivar g=glob\"\n"
    );
}

#[test]
fn block_locals_work_on_the_times_inline_path_and_in_lambdas() {
    // `n.times { }` on an integer literal is spliced inline as a native Rust
    // loop rather than becoming a real Proc, so it binds its params at its
    // own site and needs the block-local declaration applied there too.
    let result = run_ruby(
        r#"
        n = "outer"
        3.times { |i; n| n = i }
        p n

        f = ->(x; t) { t = x * 2; t }
        p f.call(5)

        q = "kept"
        [[1, 2, 3]].each { |(x, y), *r; q| q = x }
        p q
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"outer\"\n10\n\"kept\"\n");
}

#[test]
fn stdout_stderr_constants_and_globals() {
    let result = run_ruby(
        r#"
        STDOUT.puts "via const"
        $stdout.puts "via global"
        STDERR.puts "err const"
        $stderr.print "err global\n"
        puts STDOUT.write("abc\n")
        $stdout = STDERR
        puts "redirected"
        $stdout = STDOUT
        puts "back"
        puts STDOUT.inspect
        "#,
    );
    assert_eq!(
        result.stdout,
        "via const\nvia global\nabc\n4\nback\n#<IO:<STDOUT>>\n"
    );
    assert_eq!(result.stderr, "err const\nerr global\nredirected\n");
}

#[test]
fn a_statically_wrong_arity_call_raises_at_runtime_and_dead_code_stays_silent() {
    // CRuby's behavior: the error belongs to the CALL, not the program.
    let result = run_ruby(
        r##"
        module M
          def self.run!(a, b, c, d)
            "#{a}#{b}#{c}#{d}"
          end
        end
        begin
          M.run!(1, 2, false)
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        puts M.run!(1, 2, 3, 4)
        def never_called
          M.run!(1)
        end
        puts "done"
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: wrong number of arguments (given 3, expected 4)\n1234\ndone\n"
    );
}

// --- plan P-B: the core classes (File, Dir, Time, Process, ENV) ------------

/// `File`'s pure-path family: string work that never touches the disk. Every
/// expectation oracle-read from ruby 4.0.5 -- including the two that read
/// like off-by-ones (a TRAILING dot IS an extension, a LEADING one is not).
#[test]
fn file_pure_path_family() {
    let result = run_ruby(
        r#"
        puts File.basename("/home/user/notes.md")
        puts File.basename("/home/user/notes.md", ".md")
        puts File.basename("/home/user/notes.md", ".*")
        puts File.basename("/a/b/")
        puts File.basename("/")
        puts File.dirname("/home/user/notes.md")
        puts File.dirname("solo")
        puts File.dirname("/x")
        p File.extname("archive.tar.gz")
        p File.extname(".bashrc")
        p File.extname("trailing.")
        p File.extname("plain")
        p File.split("/a/b/c.rb")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "notes.md\nnotes\nnotes\nb\n/\n/home/user\n.\n/\n\".gz\"\n\"\"\n\".\"\n\"\"\n[\"/a/b\", \"c.rb\"]\n"
    );
}

/// `File.join` collapses a separator at the seam rather than doubling it, and
/// flattens a nested Array argument.
#[test]
fn file_join_collapses_separators() {
    let result = run_ruby(
        r#"
        puts File.join("a", "b", "c")
        puts File.join("a/", "b")
        puts File.join("a", "/b")
        puts File.join("a/", "/b")
        puts File.join("/a", "b")
        puts File.join("a", ["b", "c"])
        p File.absolute_path?("/abs")
        p File.absolute_path?("rel")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "a/b/c\na/b\na/b\na/b\n/a/b\na/b/c\ntrue\nfalse\n"
    );
}

/// `strftime`'s directive table, including Ruby's `-`/`_` padding flags and
/// the verbatim-unknown-directive rule.
#[test]
fn time_strftime_directives() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000).getutc
        puts t.strftime("%Y-%m-%d %H:%M:%S")
        puts t.strftime("%F %T")
        puts t.strftime("%a %A %b %B")
        puts t.strftime("%j %u %w %p %I")
        puts t.strftime("%z %Z")
        puts t.strftime("%y %C %s")
        puts t.strftime("100%% literal")
        puts t.strftime("%Q")
        jan = Time.at(1704067200).getutc
        puts jan.strftime("%m|%-m|%_m")
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2023-11-14 22:13:20\n2023-11-14 22:13:20\nTue Tuesday Nov November\n318 2 2 PM 10\n+0000 UTC\n23 20 1700000000\n100% literal\n%Q\n01|1| 1\n"
    );
}

/// A missing path raises the right `Errno::*`, with CRuby's message shape --
/// and it is catchable by its PARENT (`SystemCallError`), which is what the
/// prelude hierarchy buys.
#[test]
fn file_errors_are_real_errno_classes() {
    let result = run_ruby(
        r##"
        begin
          File.read("/definitely/not/here")
        rescue Errno::ENOENT => e
          puts "#{e.class}: #{e.message}"
        end
        p Errno::ENOENT.superclass
        p Errno::ENOENT.ancestors.include?(StandardError)
        begin
          Dir.entries("/definitely/not/here")
        rescue SystemCallError => e
          puts e.class
        end
        begin
          File.read(5)
        rescue TypeError => e
          puts "TypeError"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Errno::ENOENT: No such file or directory @ rb_sysopen - /definitely/not/here\nSystemCallError\ntrue\nErrno::ENOENT\nTypeError\n"
    );
}

/// The `File.<predicate>?` family answers false for a missing path rather
/// than raising -- and `size?` is nil for a missing OR empty file, though
/// `size` is 0 for an empty one.
#[test]
fn file_predicates_answer_rather_than_raise() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_pred_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          full = File.join(dir, "full.txt")
          empty = File.join(dir, "empty.txt")
          missing = File.join(dir, "missing.txt")
          File.write(full, "12345")
          File.write(empty, "")

          p [File.exist?(full), File.exist?(missing)]
          p [File.file?(full), File.file?(dir)]
          p [File.directory?(dir), File.directory?(full)]
          p [File.zero?(empty), File.zero?(full)]
          p File.size(full)
          p File.size(empty)
          p File.size?(full)
          p File.size?(empty)
          p File.size?(missing)
          p [File.exist?(missing), File.file?(missing), File.directory?(missing)]

          File.delete(full, empty)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[true, false]\n[true, false]\n[true, false]\n[true, false]\n5\n0\n5\nnil\nnil\n[false, false, false]\n"
    );
}

/// Whole-file read/write/readlines, including `chomp: true`.
#[test]
fn file_whole_file_io() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_rw_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          path = File.join(dir, "lines.txt")
          n = File.write(path, "alpha\nbeta\ngamma\n")
          p n
          p File.read(path)
          p File.readlines(path)
          p File.readlines(path, chomp: true)
          File.write(path, "no trailing newline")
          p File.readlines(path)
          File.delete(path)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "17\n\"alpha\\nbeta\\ngamma\\n\"\n[\"alpha\\n\", \"beta\\n\", \"gamma\\n\"]\n[\"alpha\", \"beta\", \"gamma\"]\n[\"no trailing newline\"]\n"
    );
}

/// `File.open`: the block form closes on every exit path and answers the
/// block's value. A LENGTHED read at EOF is nil where a whole-rest read is
/// `""` -- the asymmetry a `while chunk = f.read(n)` loop relies on.
#[test]
fn file_open_block_form_and_positioned_reads() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_open_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          path = File.join(dir, "counted.txt")
          File.open(path, "w") { |f| f.print "ABCDEFGHIJ" }
          p File.read(path)

          File.open(path, "r") do |f|
            p f.read(5)
            p f.read(5)
            p f.read(5)
            f.rewind
            p f.read(2)
            p f.tell
            f.seek(0)
            p f.read
            p f.eof?
          end

          p File.open(path, "r") { |f| f.read(3) }

          h = File.open(path, "r")
          p h.closed?
          h.close
          p h.closed?
          begin
            h.read
          rescue IOError => e
            puts "IOError: #{e.message}"
          end

          File.delete(path)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ABCDEFGHIJ\"\n\"ABCDE\"\n\"FGHIJ\"\nnil\n\"AB\"\n2\n\"ABCDEFGHIJ\"\ntrue\n\"ABC\"\nfalse\ntrue\nIOError: closed stream\n"
    );
}

/// `File.open`'s block form closes the file even when the block RAISES --
/// that ensure is the whole reason the idiom exists.
#[test]
fn file_open_closes_even_when_the_block_raises() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_raise_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          path = File.join(dir, "x.txt")
          File.write(path, "data")
          handle = nil
          begin
            File.open(path, "r") do |f|
              handle = f
              raise "boom"
            end
          rescue RuntimeError => e
            puts "rescued: #{e.message}"
          end
          p handle.closed?
          File.delete(path)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: boom\ntrue\n");
}

/// `Dir` listing: `entries` includes `.`/`..`, `children` does not.
#[test]
fn dir_listing_distinguishes_entries_from_children() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_list_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          File.write(File.join(dir, "a.rb"), "")
          File.write(File.join(dir, "b.txt"), "")
          Dir.mkdir(File.join(dir, "sub"))

          p Dir.children(dir).sort
          p Dir.entries(dir).sort
          p Dir.exist?(dir)
          p Dir.exist?(File.join(dir, "a.rb"))
          p Dir.exist?("/definitely/not/here")
          p Dir.empty?(File.join(dir, "sub"))
          p Dir.pwd.start_with?("/")

          Dir.rmdir(File.join(dir, "sub"))
          File.delete(File.join(dir, "a.rb"), File.join(dir, "b.txt"))
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a.rb\", \"b.txt\", \"sub\"]\n[\".\", \"..\", \"a.rb\", \"b.txt\", \"sub\"]\ntrue\nfalse\nfalse\ntrue\ntrue\n"
    );
}

/// `Dir.glob`: `*` within a segment, `**` across them, `?`, `{a,b}`
/// alternation, and Ruby's hidden-file rule (a leading `.` is invisible to a
/// wildcard).
#[test]
fn dir_glob_semantics() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_glob_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          Dir.chdir(dir) do
            Dir.mkdir("sub")
            File.write("x.rb", "")
            File.write("y.txt", "")
            File.write(".hidden", "")
            File.write("sub/z.rb", "")

            p Dir.glob("*.rb").sort
            p Dir.glob("**/*.rb").sort
            p Dir.glob("sub/*.rb")
            p Dir.glob("*.{rb,txt}").sort
            p Dir.glob("?.rb")
            p Dir["*.rb"]
            p Dir.glob("*").sort
            p Dir.glob("*").include?(".hidden")
            p Dir.glob(".*").include?(".hidden")

            File.delete("x.rb", "y.txt", ".hidden", "sub/z.rb")
            Dir.rmdir("sub")
          end
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"x.rb\"]\n[\"sub/z.rb\", \"x.rb\"]\n[\"sub/z.rb\"]\n[\"x.rb\", \"y.txt\"]\n[\"x.rb\"]\n[\"x.rb\"]\n[\"sub\", \"x.rb\", \"y.txt\"]\nfalse\ntrue\n"
    );
}

/// `Dir.chdir`'s block form restores the previous directory afterwards.
#[test]
fn dir_chdir_block_form_restores_the_previous_directory() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_chdir_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          before = Dir.pwd
          Dir.chdir(dir) do
            p Dir.pwd != before
          end
          p Dir.pwd == before
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\n");
}

/// `Dir.mkdir`/`rmdir` round-trip, and `File.rename`/`delete`.
#[test]
fn dir_and_file_mutation_round_trips() {
    let result = run_ruby(
        r##"
        dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_mut_#{Process.pid}")
        Dir.mkdir(dir)
        begin
          fresh = File.join(dir, "fresh")
          Dir.mkdir(fresh)
          p Dir.exist?(fresh)
          Dir.rmdir(fresh)
          p Dir.exist?(fresh)

          a = File.join(dir, "a.txt")
          b = File.join(dir, "b.txt")
          File.write(a, "x")
          File.rename(a, b)
          p [File.exist?(a), File.exist?(b)]
          p File.delete(b)
          p File.exist?(b)
        ensure
          Dir.rmdir(dir)
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nfalse\n[false, true]\n1\nfalse\n");
}

#[test]
fn alias_under_a_static_modifier_and_if_elsif() {
    // A statically-literal `if`/`unless` guard on an `alias` is folded at
    // definition time: the selected branch's alias is registered.
    let result = run_ruby(
        r#"
        class C
          def one = 1
          def two = 2
          alias uno one if true
          alias dos two unless false
          alias never one if false
          if false
            alias chosen one
          elsif true
            alias chosen two
          end
        end
        puts C.new.uno
        puts C.new.dos
        puts C.new.chosen
        puts C.new.respond_to?(:never)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\n2\nfalse\n");
}

#[test]
fn format_directives_named_positional_star_and_alternate_form() {
    let result = run_ruby(
        r#"
        puts format("%<name>s is %<age>d", name: "Ada", age: 36)
        puts format("%#b / %#x", 10, 255)
        puts format("%*d|", 5, 42)
        puts format("%2$s %1$s", "world", "hello")
        puts format("%a", 0.5)
        puts("%<x>05.2f" % { x: 3.14159 })
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Ada is 36\n0b1010 / 0xff\n   42|\nhello world\n0x1p-1\n03.14\n"
    );
}

#[test]
fn pack_float_native_and_encoding_directives() {
    let result = run_ruby(
        r#"
        p [1.5].pack("D").bytes
        p [1.5].pack("G").bytes
        p [3.14].pack("d").unpack("d")
        p [1].pack("l!").bytesize
        p [1].pack("i").bytesize
        p [1].pack("j").bytesize
        p ["hello world"].pack("M")
        p ["hi there folks"].pack("u").unpack("u")
        p(["hi there folks"].pack("u").unpack("u") == ["hi there folks"])
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[0, 0, 0, 0, 0, 0, 248, 63]\n[63, 248, 0, 0, 0, 0, 0, 0]\n[3.14]\n8\n4\n8\n\"hello world=\\n\"\n[\"hi there folks\"]\ntrue\n"
    );
}

#[test]
fn raise_cause_is_three_state() {
    // CRuby distinguishes three states with a Qundef/Qnil/value sentinel
    // (rb_f_raise, eval.c:740), and the first two mean OPPOSITE things: an
    // omitted `cause:` chains automatically from $!, while `cause: nil`
    // SUPPRESSES that chaining. Modeling this as Option<NodeId> would lower
    // `cause: nil` to None and silently chain anyway -- so both are asserted
    // here, along with the TypeError and circular-cause rejections.
    let result = run_ruby(
        r##"
        def blow(m); raise "top", cause: ArgumentError.new(m); end
        begin
          blow("root")
        rescue => e
          p e.cause
          puts e.message
        end
        begin
          begin
            raise ArgumentError, "inner"
          rescue ArgumentError
            raise "outer"
          end
        rescue => e
          p e.cause
        end
        begin
          begin
            raise ArgumentError, "inner2"
          rescue ArgumentError
            raise "outer2", cause: nil
          end
        rescue => e
          p e.cause
        end
        begin
          raise "x", cause: 5
        rescue TypeError => e
          puts "TypeError: #{e.message}"
        end
        begin
          a = RuntimeError.new("a")
          b = RuntimeError.new("b")
          begin
            raise a, cause: b
          rescue; end
          raise b, cause: a
        rescue ArgumentError => e
          puts "ArgumentError: #{e.message}"
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "#<ArgumentError: root>\n\
         top\n\
         #<ArgumentError: inner>\n\
         nil\n\
         TypeError: exception object expected\n\
         ArgumentError: circular causes\n",
    );
}

/// A non-builtin feature still can't be resolved from a non-top-level
/// position -- there is genuinely a file to splice and nowhere to splice it.
#[test]
fn require_of_a_file_feature_is_still_a_clean_rejection_off_top_level() {
    let err = zeo::compile_to_rust("if true\n  require \"some_lib\"\nend\n").unwrap_err();
    assert!(
        err.contains("only supported as a top-level statement"),
        "unexpected error: {err}"
    );
}

/// A `.so` require resolves through the static-ext table rather than the
/// filesystem -- zeo is a Ruby built with `--with-static-linked-ext`, and
/// CRuby registers static exts under `"<feature>.so"` (`load.c:1161`).
#[test]
fn an_explicit_so_require_resolves_a_statically_linked_extension() {
    let result = run_ruby("require \"strscan.so\"\np StringScanner.new(\"ab\").scan(/a/)\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"a\"\n");

    // A .so naming no static ext is still a clean LoadError, not a silent no-op.
    let err = zeo::compile_to_rust("require \"nope.so\"\n").unwrap_err();
    assert!(
        err.contains("cannot load such file -- nope.so"),
        "unexpected: {err}"
    );
}

/// A feature with no Ruby half falls through to the static-ext table, which is
/// the ordinary case for most extensions.
#[test]
fn a_feature_with_no_ruby_half_falls_through_to_the_static_ext_table() {
    let result = run_ruby("require \"base64\"\np Base64.encode64(\"hi\")\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "\"aGk=\\n\"\n");
}

#[test]
fn file_and_dir_surface_gaps() {
    // Dir.glob with an ABSOLUTE-path pattern + a `*` wildcard (used to return
    // [] because the walk read "" instead of "/"); FNM_DOTMATCH yields "." and
    // dotfiles but never ".."; File.mkfifo returns 0 and creates a FIFO;
    // File#lstat returns a File::Stat; File.exists? was removed in Ruby 3.2.
    let result = run_ruby(
        r##"
        d = "/tmp/sp_e2e_fdir_#{Process.pid}"
        Dir.mkdir(d) unless Dir.exist?(d)
        File.write("#{d}/a1", ""); File.write("#{d}/a2", ""); File.write("#{d}/.hid", "")
        p Dir.glob("#{d}/*").map { |x| x.sub("#{d}/", "") }.sort
        p Dir.glob("#{d}/*", File::FNM_DOTMATCH).map { |x| x.sub("#{d}/", "") }.sort
        fifo = "#{d}/f"
        p File.mkfifo(fifo)
        p File.stat(fifo).ftype
        p File.open("#{d}/a1") { |f| f.lstat.class }
        r = (begin; File.exists?("#{d}"); rescue => e; e.class; end); p r
        File.delete("#{d}/a1"); File.delete("#{d}/a2"); File.delete("#{d}/.hid"); File.delete(fifo)
        Dir.rmdir(d)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[\"a1\", \"a2\"]\n\
         [\".\", \".hid\", \"a1\", \"a2\"]\n\
         0\n\
         \"fifo\"\n\
         File::Stat\n\
         NoMethodError\n"
    );
}

#[test]
fn io_instance_method_surface() {
    // ungetbyte pushes a byte back for the next read; binmode/binmode?,
    // autoclose=/autoclose?, to_io (self), close_on_exec?, pread/pwrite at a
    // fixed offset, advise (nil), close_write on a read-only file (IOError),
    // reopen (rebinds to another file), each_codepoint.
    let result = run_ruby(
        r##"
        pth = "/tmp/sp_e2e_io_#{Process.pid}.tmp"
        File.write(pth, "hi\n")
        File.open(pth) do |f|
          p f.readbyte
          f.ungetbyte(104)
          p f.readbyte
          p f.binmode?
          f.binmode
          p f.binmode?
          p f.to_io.equal?(f)
          p f.close_on_exec?
          p f.pread(2, 0)
          p f.advise(:normal)
          p((f.autoclose = false))
          p f.autoclose?
          r = (begin; f.close_write; rescue => e; e.class; end); p r
        end
        File.open(pth, "r+") { |f| p f.pwrite("X", 0) }
        p File.read(pth)
        File.write("/tmp/sp_e2e_io2_#{Process.pid}.tmp", "other")
        File.open(pth) { |f| File.open("/tmp/sp_e2e_io2_#{Process.pid}.tmp") { |g| f.reopen(g); p f.read } }
        File.open(pth) { |f| cps = []; f.each_codepoint { |c| cps << c }; p cps }
        File.delete(pth); File.delete("/tmp/sp_e2e_io2_#{Process.pid}.tmp")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "104\n104\nfalse\ntrue\ntrue\ntrue\n\"hi\"\nnil\nfalse\nfalse\nIOError\n1\n\"Xi\\n\"\n\"other\"\n[88, 105, 10]\n"
    );
}

#[test]
fn tcp_server_and_socket_round_trip() {
    // require "socket" activates TCPServer/TCPSocket. A single-threaded round
    // trip: connect first (queues in the listen backlog), accept, then exchange
    // bytes via the inherited IO surface (write/gets/read on TCPSocket < IO).
    let result = run_ruby(
        r#"
        require "socket"
        server = TCPServer.new("127.0.0.1", 0)
        p server.addr[1] > 0
        p server.class
        c = TCPSocket.new("127.0.0.1", server.addr[1])
        s = server.accept
        p s.class
        s.write "hello\n"
        p c.gets
        c.write "back\n"
        p s.gets
        s.print "tail"
        s.close
        p c.read
        c.close
        server.close
        p server.closed?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nTCPServer\nTCPSocket\n\"hello\\n\"\n\"back\\n\"\n\"tail\"\ntrue\n"
    );
}

#[test]
fn symbol_to_proc_arity_and_io_fcntl() {
    // :name.to_proc reports arity -2 (receiver + optional args); IO#fcntl runs
    // the raw syscall (F_GETFD reads the close-on-exec flag on the open file).
    let result = run_ruby(
        r##"
        p :upcase.to_proc.arity
        p ["x", "y"].map(&:upcase)
        pth = "/tmp/sp_e2e_fcntl_#{Process.pid}.tmp"
        File.write(pth, "hi")
        File.open(pth) { |f| p(f.fcntl(1, 0).class) }
        File.delete(pth)
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "-2\n[\"X\", \"Y\"]\nInteger\n");
}
