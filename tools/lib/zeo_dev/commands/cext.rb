# frozen_string_literal: true

require "fileutils"
require "tmpdir"

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
      SUBCOMMANDS = %w[sync patch].freeze

      def self.summary = "manage the vendored MRI C API headers"

      def self.banner = <<~TEXT
        usage: zeo-dev cext <subcommand> [options]

        subcommands:
          sync [--check]      rebuild the headers from upstream + patches/
          patch <name>        record the working tree's deviation as a patch
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
          File.write(out, rewrite_prefixes(diff.stdout, want))
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
