# frozen_string_literal: true

require "fileutils"
require "tmpdir"
require "set"

module ZeoDev
  module Commands
    # Manage the vendored MRI C API headers under `crates/zeo-rt/cext/`.
    #
    #   cext sync [--check]
    #   cext patch <name>
    #
    # zeo is source-compatible with MRI and ABI-incompatible with it: a gem's
    # `ext/**/*.c` compiles against MRI's own headers, and a prebuilt MRI
    # `.so` never loads. The headers are therefore upstream verbatim plus a
    # patch series that turns every layout-reading macro into a call, because
    # a zeo heap object is an opaque handle and has no `struct RString` behind
    # it.
    #
    # `sync` rebuilds the tree from that sum, so the vendored bytes are always
    # exactly `upstream(rev) + patches/`. `--check` proves it without writing,
    # which is what CI runs -- a hand-edit to a vendored header is drift, and
    # the way to keep one is `cext patch`.
    class Cext < Cli
      SUBCOMMANDS = %w[sync patch api].freeze

      def self.summary = "manage the vendored MRI C API headers"

      def self.banner = <<~TEXT
        usage: zeo-dev cext <subcommand> [options]

        subcommands:
          sync [--check]      rebuild the headers from upstream + patches/
          patch <name>        record the working tree's deviation as a patch
          api [--check]       re-record the rb_* census and regenerate stubs
      TEXT

      def defaults = { check: false }

      def options(o)
        o.on("--check", "verify without writing; nonzero on drift") { opts[:check] = true }
      end

      def run
        sub = args.shift
        raise Error, self.class.banner unless SUBCOMMANDS.include?(sub)

        send("cmd_#{sub}")
      end

      private

      CEXT = "crates/zeo-rt/cext"

      def cext_dir = File.join(ROOT, CEXT)
      def include_dir = File.join(cext_dir, "include")
      def patch_dir = File.join(cext_dir, "patches")

      def entry
        @entry ||= Manifest.load.find("ruby", group: :headers) ||
                   raise(Error, "upstream.rb has no `headers \"ruby\"` entry")
      end

      def patches = Dir.glob(File.join(patch_dir, "*.patch")).sort

      # Upstream's `include/` at the pinned rev, with the patch series applied
      # on top, materialized in a scratch directory.
      def build_expected(dest)
        checkout = Vendor.fetch_checkout(entry, entry.rev)
        FileUtils.rm_rf(dest)
        Vendor.copy_tree(Vendor.source_root(checkout, entry), dest)
        patches.each do |p|
          res = Exec.run(["git", "apply", "--whitespace=nowarn", p], chdir: dest)
          raise Error, "#{File.basename(p)} does not apply to upstream #{entry.tag}" unless res.success?
        end
        dest
      end

      API_TSV = "conformance/cext-api.tsv"
      STUBS_RS = "crates/zeo-rt/src/cext/stubs.rs"
      CEXT_SRC = "crates/zeo-rt/src/cext"

      # Every `rb_*`/`ruby_*` an extension can link against, from clang's own
      # AST rather than a regex over the headers.
      #
      # A regex cannot tell `RSTRING_LEN` -- a `static inline` an extension
      # compiles into its own object -- from `rb_str_new`, which it expects to
      # find at link time. Only the second kind needs an implementation or a
      # stub, and clang already knows which is which.
      def cmd_api
        decls = scan_api
        tsv = render_tsv(decls)
        stubs = render_stubs(decls)
        if opts[:check]
          stubs = rustfmt_would_write(stubs)
          drift = []
          drift << API_TSV unless File.exist?(File.join(ROOT, API_TSV)) &&
                                  File.read(File.join(ROOT, API_TSV)) == tsv
          drift << STUBS_RS unless File.exist?(File.join(ROOT, STUBS_RS)) &&
                                   File.read(File.join(ROOT, STUBS_RS)) == stubs
          unless drift.empty?
            warn "cext: stale -- run `zeo-dev cext api`: #{drift.join(", ")}"
            return 1
          end
          puts "cext: #{decls.size} linkable symbols, #{decls.count { |d| d[:status] == "stub" }} stubbed"
          return 0
        end
        Tsv.write(File.join(ROOT, API_TSV), tsv)
        Tsv.write(File.join(ROOT, STUBS_RS), stubs)
        # `cargo fmt --all` reaches generated files too, so the generator has
        # to emit what rustfmt would. Otherwise the next `fmt` rewrites the
        # file and `--check` reports drift the generator cannot fix.
        Exec.run(["rustfmt", "--edition", "2024", File.join(ROOT, STUBS_RS)])
        puts "cext: wrote #{API_TSV} and #{STUBS_RS} " \
             "(#{decls.size} symbols, #{decls.count { |d| d[:status] == "stub" }} stubbed)"
        0
      end

      # What `rustfmt` makes of a rendering, so `--check` compares like with
      # like.
      def rustfmt_would_write(text)
        Dir.mktmpdir("zeo-cext-fmt") do |tmp|
          f = File.join(tmp, "stubs.rs")
          File.write(f, text)
          Exec.run(["rustfmt", "--edition", "2024", f])
          File.read(f)
        end
      end

      # Symbols the runtime already exports, read off its own `#[no_mangle]`
      # attributes. Asking the source rather than keeping a list is what stops
      # the two drifting.
      def implemented
        @implemented ||= Dir.glob(File.join(ROOT, CEXT_SRC, "*.rs")).reject do |f|
          # `stubs.rs` is this command's own output. Counting its stubs as
          # implementations would make the second run report every one of
          # them as done.
          File.basename(f) == "stubs.rs"
        end.flat_map do |f|
          # Two spellings reach the same place: a hand-written export, and
          # the `cext_fn!` macro that wraps a `Result` body into one.
          src = File.read(f)
          src.scan(/#\[unsafe\(no_mangle\)\]\s*(?:pub\s+)?(?:unsafe\s+)?extern "C" fn (\w+)/) +
            src.scan(/^\s*fn (rb_\w+|ruby_\w+)\s*\(/)
        end.flatten.to_set
      end

      def scan_api
        Dir.mktmpdir("zeo-cext-api") do |tmp|
          tu = File.join(tmp, "all.c")
          File.write(tu, "#include <ruby.h>\n")
          res = Exec.run([ENV["CC"] || "cc", "-fsyntax-only", "-Xclang", "-ast-dump",
                          "-fno-color-diagnostics",
                          "-I", File.join(ROOT, "crates/zeo-rt/cext/config"),
                          "-I", File.join(ROOT, "crates/zeo-rt/cext/include"), tu],
                         capture_stdout: true)
          raise Error, "clang could not parse the vendored headers" if res.stdout.empty?

          parse_ast(res.stdout)
        end
      end

      # A linkable declaration is one clang did NOT mark `static inline`: that
      # marker is the whole difference between a macro-shaped helper the gem
      # compiles itself and a symbol it expects zeo to provide.
      FN = /FunctionDecl 0x\h+ (?:prev 0x\h+ )?<[^>]*> (?:line|col):\S+ (?:used |referenced )?((?:rb|ruby)_\w+) '([^']*)'\s*$/
      VAR = /VarDecl 0x\h+ (?:prev 0x\h+ )?<[^>]*> (?:line|col):\S+ (?:used |referenced )?((?:rb|ruby)_\w+) '([^']*)'(?::'[^']*')? extern\s*$/

      def parse_ast(text)
        seen = {}
        text.each_line do |line|
          if (m = FN.match(line))
            seen[m[1]] ||= { name: m[1], kind: "fn", sig: m[2] }
          elsif (m = VAR.match(line))
            seen[m[1]] ||= { name: m[1], kind: "var", sig: m[2] }
          end
        end
        seen.values.sort_by { |d| d[:name] }.each do |d|
          d[:status] = implemented.include?(d[:name]) ? "zeo" : "stub"
        end
      end

      def render_tsv(decls)
        rows = decls.map { |d| [d[:name], d[:kind], d[:status], d[:sig]].join("\t") }
        <<~HEAD + rows.join("\n") + "\n"
          # Every rb_*/ruby_* symbol a C extension can link against, from clang's AST
          # of the vendored headers. Regenerate with `tools/zeo-dev cext api`.
          #
          # status=zeo  the runtime exports it
          # status=stub crates/zeo-rt/src/cext/stubs.rs raises NotImplementedError
          symbol\tkind\tstatus\tsignature
        HEAD
      end

      # A stub takes no arguments and never returns.
      #
      # That looks like a prototype mismatch, and by the C standard it is. It
      # is safe on both ABIs zeo targets: the caller passes arguments in
      # registers and on a stack it cleans up itself, the callee reads none of
      # them, and it never returns -- so there is no return value to disagree
      # about and no frame to unwind. Writing 800 correct signatures instead
      # would buy nothing: not one of these functions runs.
      def render_stubs(decls)
        stubs = decls.select { |d| d[:kind] == "fn" && d[:status] == "stub" }
        vars = decls.select { |d| d[:kind] == "var" && d[:status] == "stub" }
        body = stubs.map { |d| <<~RS }.join
          #[unsafe(no_mangle)]
          pub extern "C" fn #{d[:name]}() -> ! {
              unimplemented(#{d[:name].inspect})
          }
        RS
        globals = vars.map { |d| <<~RS }.join
          #[unsafe(no_mangle)]
          pub static #{d[:name]}: Global = unfilled();
        RS
        table = vars.map { |d| "    (#{d[:name].inspect}, &#{d[:name]})," }.join("\n")
        <<~RS
          //! One loud stub per `rb_*` zeo does not answer yet.
          //!
          //! Generated by `tools/zeo-dev cext api`. Do not edit.
          //!
          //! # Why every one of them exists
          //!
          //! A gem's `ext/**/*.c` is compiled and linked as a whole. One
          //! reference to a function zeo has not written yet would fail the
          //! LINK, with a message naming a symbol and no gem, no file and no
          //! line -- and it would fail even when the call sits on a branch the
          //! program never takes. So every declared symbol resolves, and one
          //! that is not implemented raises when it is CALLED, naming itself.
          //!
          //! A stub disappears the moment the real function is written: the
          //! generator reads `#[unsafe(no_mangle)]` out of the other files in
          //! this module, so implementing one is all it takes.
          //!
          //! # The signatures
          //!
          //! A stub takes no arguments and never returns. By the C standard
          //! that is a prototype mismatch; on both ABIs zeo targets it is
          //! safe. The caller passes arguments in registers and on a stack it
          //! cleans up itself, the callee reads none of them, and it never
          //! returns -- so there is no return value to disagree about and no
          //! frame to unwind. Writing #{stubs.size} correct signatures would
          //! buy nothing: not one of these functions runs.
          //!
          //! # The globals
          //!
          //! `rb_cObject`, `rb_eArgError` and #{vars.size - 2} others are
          //! `VALUE` variables an extension reads directly, so each is a real
          //! symbol rather than a stub. Each starts as `Qundef` and the
          //! loader fills it before any `Init_` runs; [`GLOBALS`] is what it
          //! walks, and `every_global_is_filled` is what proves none was
          //! missed. Nothing can read one before the loader runs, because
          //! nothing has loaded.

          use std::sync::atomic::{AtomicUsize, Ordering};

          /// A `VALUE` variable C reads by name. `AtomicUsize` so Rust can
          /// write it; C sees a plain `VALUE`, which is the same word.
          pub type Global = AtomicUsize;

          /// Every global starts here, and the loader is what moves it.
          const fn unfilled() -> Global {
              AtomicUsize::new(crate::cext::value::Q_UNDEF)
          }

          /// Every `VALUE` global, by the name C spells.
          pub static GLOBALS: &[(&str, &Global)] = &[
          #{table}
          ];

          /// Read one, for a caller that has the name rather than the symbol.
          pub fn global(name: &str) -> Option<usize> {
              GLOBALS
                  .iter()
                  .find(|(n, _)| *n == name)
                  .map(|(_, g)| g.load(Ordering::Relaxed))
          }

          /// Raise, naming the symbol the extension asked for.
          fn unimplemented(what: &'static str) -> ! {
              crate::cext::jmp::raise(crate::dispatch::raise_error(
                  "NotImplementedError",
                  format!("{what} is not implemented by zeo"),
              ))
          }

          #{globals.chomp}

          #{body.chomp}
        RS
      end

      def cmd_sync
        Dir.mktmpdir("zeo-cext") do |tmp|
          want = build_expected(File.join(tmp, "include"))
          if opts[:check]
            return report_drift(want) unless Vendor.dirs_equal?(want, include_dir)

            puts "cext: #{Vendor.list_files(want).size} headers match upstream " \
                 "#{entry.tag} + #{patches.size} patch(es)"
            return 0
          end
          FileUtils.rm_rf(include_dir)
          Vendor.copy_tree(want, include_dir)
          puts "cext: vendored #{Vendor.list_files(include_dir).size} headers from " \
               "#{entry.url} @ #{entry.tag} + #{patches.size} patch(es)"
        end
        0
      end

      # Name every file that differs, not just the count: a header tree is too
      # big for a bare "drift" to be actionable.
      def report_drift(want)
        have = Vendor.list_files(include_dir)
        expected = Vendor.list_files(want)
        (expected - have).each { |r| warn "  missing: #{r}" }
        (have - expected).each { |r| warn "  extra:   #{r}" }
        (expected & have).each do |r|
          a = File.binread(File.join(want, r))
          b = File.binread(File.join(include_dir, r))
          warn "  changed: #{r}" if a != b
        end
        warn "cext: the vendored headers are not upstream #{entry.tag} + patches/ " \
             "-- run `zeo-dev cext patch <name>` to keep an edit, or `cext sync` to discard it"
        1
      end

      # Fold the working tree's whole deviation into one new patch. The series
      # is applied in name order, so a later patch may depend on an earlier
      # one; recording the deviation as a single hunk set keeps that honest.
      def cmd_patch
        name = args.shift or raise Error, "cext patch needs a name, e.g. `rstring-is-opaque`"
        Dir.mktmpdir("zeo-cext") do |tmp|
          want = build_expected(File.join(tmp, "include"))
          if Vendor.dirs_equal?(want, include_dir)
            puts "cext: nothing to record -- the tree already matches upstream + patches/"
            return 0
          end
          diff = Exec.run(["git", "diff", "--no-index", "--src-prefix=a/", "--dst-prefix=b/",
                           want, include_dir], capture_stdout: true)
          # `git diff --no-index` exits 1 when the trees differ, which is the
          # whole reason it was run.
          raise Error, "git diff failed: #{diff.stderr.strip}" if diff.stdout.empty?

          out = File.join(patch_dir, "#{format("%04d", patches.size + 1)}-#{name}.patch")
          FileUtils.mkdir_p(patch_dir)
          # `git apply` ignores anything before the first `diff --git`, so the
          # patch carries its own reason. A headerless one says nothing about
          # WHY a vendored header reads the way it does.
          header = "Subject: #{name.tr("-", " ")}\n\nTODO: say what this changes and why.\n\n"
          File.write(out, header + rewrite_prefixes(diff.stdout, want))
          puts "cext: wrote #{out.delete_prefix("#{ROOT}/")}"
        end
        0
      end

      # `--no-index` writes absolute scratch paths into the header lines. A
      # patch that names a tmpdir cannot be re-applied, so rewrite both sides
      # to the plain relative paths `git apply` expects inside the tree.
      def rewrite_prefixes(text, want)
        text.gsub("a#{want}/", "a/").gsub("b#{include_dir}/", "b/")
            .gsub("#{want}/", "").gsub("#{include_dir}/", "")
      end
    end
  end
end
