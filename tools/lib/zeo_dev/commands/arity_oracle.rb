# frozen_string_literal: true

require "json"

module ZeoDev
  module Commands
    # Records what CRuby reports for every class zeo declares, into
    # `conformance/builtin-arity.tsv`.
    #
    # The dump is checked in so `crates/zeo-tests/tests/builtin_arity.rs` can
    # gate arity drift on a machine with no ruby. Regenerating is an explicit
    # act; `--check` re-runs the oracle and fails if the committed copy is
    # stale, for a CI job that does have the pinned ruby.
    #
    # The join key is the `ClassId` const ident. It is the only token the DSL
    # header and `zeo-abi`'s `BUILTINS` table share -- the header spells the
    # class `Stat`, while the Ruby name is `File::Stat` and only `BUILTINS`
    # knows that, along with the `require` that exposes it.
    #
    # Reading the DSL needs a `syn` parse of the runtime's own source, which
    # is the one thing ruby cannot do, so `zeo dev scan-decls` supplies both
    # halves as JSON and the join happens here.
    class ArityOracle < Cli
      DUMP = "conformance/builtin-arity.tsv"
      SCRIPT = "tools/builtin_arity_oracle.rb"

      def self.summary = "record conformance/builtin-arity.tsv from the oracle"
      def self.banner = "usage: zeo-dev arity-oracle [--check]"

      def defaults = { check: false }

      def options(o)
        o.on("--check", "fail if the committed copy is stale, and write nothing") do
          opts[:check] = true
        end
      end

      def run
        expect_no_args!
        scan = read_scan
        manifest, unknown = build_manifest(scan)
        dump = run_oracle(manifest)
        # A header whose ClassId has no BUILTINS row. Recorded in the dump
        # rather than only on stderr, so the set is reviewable in the diff.
        unknown.each { |c| dump += "#{Tsv.row("!", c, "no-abi-row", "-")}\n" }

        target = File.join(ROOT, DUMP)
        return Tsv.check(target, dump, label: DUMP) if opts[:check]

        Tsv.write(target, dump)
        asked = scan["decls"].map { |d| d["class_const"] }.uniq.size
        puts "wrote #{DUMP} (#{asked} classes asked, #{scan["decls"].size} declarations, " \
             "#{dump.lines.count} lines)"
        puts "  #{unknown.size} header(s) have no zeo-abi BUILTINS row" unless unknown.empty?
        0
      end

      private

      def read_scan
        zeo = Ruby.build_zeo!
        res = Exec.run([zeo, "dev", "scan-decls", ROOT], capture_stdout: true)
        raise Error, "zeo dev scan-decls failed: #{res.stderr}" unless res.success?

        JSON.parse(res.stdout)
      end

      # Only classes zeo actually declares methods on are worth asking about.
      def build_manifest(scan)
        abi = scan["abi"]
        wanted = scan["decls"].map { |d| d["class_const"] }.uniq.sort
        manifest = +""
        unknown = []
        wanted.each do |const|
          c = abi[const]
          if c.nil?
            unknown << const
            next
          end
          manifest << Tsv.row("C", const, c["ruby_name"], c["is_module"], c["feature"] || "-") << "\n"
        end
        [manifest, unknown]
      end

      def run_oracle(manifest)
        res = Exec.run([Ruby.oracle, File.join(ROOT, SCRIPT)],
                       stdin: manifest, chdir: ROOT, capture_stdout: true)
        raise Error, "the ruby oracle exited with #{res.code.inspect}" unless res.success?

        res.stdout
      end
    end
  end
end
