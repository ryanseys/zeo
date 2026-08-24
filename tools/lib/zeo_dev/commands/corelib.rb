# frozen_string_literal: true

require "digest"
require "fileutils"
require "json"

module ZeoDev
  module Commands
    # The Ruby files CRuby compiles INTO the interpreter, vendored verbatim.
    #
    #   corelib verify          re-hash the committed files against the lock
    #   corelib sync [--check]  re-vendor them from the pinned rev
    #   corelib census          which upstream corelib files are eligible
    #
    # Two checks with different reach, and the split is the point.
    #
    # `verify` is OFFLINE. It re-hashes `crates/zeo/corelib/*.rb` and compares
    # to `upstream.lock`, so it runs in CI, in a release tarball, and on a
    # machine with no network -- and it is the only one a downstream consumer
    # can run. It proves the committed bytes are the bytes that were locked.
    #
    # `sync --check` needs the NETWORK. It fetches `ruby/ruby` at the pinned
    # rev and compares the upstream bytes to the committed ones, which is what
    # proves the locked bytes are upstream's. Run it when the pin moves.
    #
    # The lock also records each file's git blob OID -- `sha1("blob <n>\0" +
    # bytes)`, which is what GitHub's contents API answers -- so a reader can
    # verify a file against github.com with one request and no clone:
    #
    #   curl -s https://api.github.com/repos/ruby/ruby/contents/nilclass.rb\
    #     ?ref=<rev> | jq -r .sha
    class Corelib < Cli
      SUBCOMMANDS = %w[verify sync census].freeze

      def self.summary = "verify or re-vendor the corelib ruby"

      def self.banner
        <<~USAGE
          usage: zeo-dev corelib <subcommand>

            verify          re-hash the committed files against upstream.lock (offline)
            sync [--check]  re-vendor from the pinned rev (network)
            census          list which upstream corelib files are eligible
        USAGE
      end

      def defaults = { check: false }

      def options(o)
        o.on("--check", "verify without writing; nonzero on drift") { opts[:check] = true }
      end

      def run
        sub = args.shift
        raise Error, self.class.banner unless SUBCOMMANDS.include?(sub)

        send(:"cmd_#{sub}")
      end

      private

      def manifest = @manifest ||= Manifest.load(ROOT)

      def entry
        manifest.group(:corelib).first or raise Error, "upstream.rb declares no corelib entry"
      end

      def dir = File.join(ROOT, Manifest::CORELIB_DIR)

      # Offline: the committed bytes against the lock's digests.
      def cmd_verify
        locked = JSON.parse(File.read(manifest.lock_path))["corelib"] || []
        rows = locked.flat_map { |e| e["files"] || [] }
        if rows.empty?
          warn "corelib: upstream.lock records no corelib files"
          return 1
        end
        bad = 0
        # A name in `upstream.rb` with no row here was never vendored -- see
        # `Manifest.corelib_digests`. Caught before the hashes, because a
        # missing file is a missing check rather than a failing one.
        (entry.files.sort - rows.map { |r| r["path"] }).each do |rel|
          warn "corelib: #{rel} is named in upstream.rb but not vendored -- run `corelib sync`"
          bad += 1
        end
        rows.each do |row|
          path = File.join(dir, row["path"])
          unless File.file?(path)
            warn "corelib: #{row["path"]} is MISSING"
            bad += 1
            next
          end
          bytes = File.binread(path)
          sha = Digest::SHA256.hexdigest(bytes)
          blob = Manifest.blob_oid(bytes)
          if sha == row["sha256"] && blob == row["blob"]
            puts "corelib: #{row["path"]} ok (blob #{blob[0, 12]})"
          else
            warn "corelib: #{row["path"]} DOES NOT MATCH upstream.lock"
            warn "  locked sha256 #{row["sha256"]}"
            warn "  actual sha256 #{sha}"
            bad += 1
          end
        end
        return 1 if bad.positive?

        puts "corelib: #{rows.size} file(s) match #{entry.github} @ #{entry.tag} (#{entry.short_rev})"
        0
      end

      # Network: upstream at the pinned rev against the committed bytes.
      def cmd_sync
        checkout = Vendor.fetch_checkout(entry, entry.rev)
        drift = 0
        entry.files.sort.each do |rel|
          upstream = File.join(Vendor.source_root(checkout, entry), rel)
          raise Error, "#{entry.github} @ #{entry.tag} has no #{rel}" unless File.file?(upstream)

          bytes = File.binread(upstream)
          reject_primitives(rel, bytes)
          dest = File.join(dir, rel)
          if opts[:check]
            same = File.file?(dest) && File.binread(dest) == bytes
            puts(same ? "corelib: #{rel} is upstream #{entry.tag}" : "corelib: #{rel} DIFFERS from upstream #{entry.tag}")
            drift += 1 unless same
          else
            FileUtils.mkdir_p(File.dirname(dest))
            File.binwrite(dest, bytes)
            puts "corelib: vendored #{rel} (#{bytes.bytesize} bytes)"
          end
        end
        return drift.zero? ? 0 : 1 if opts[:check]

        # Re-loaded, not reused: the digests are derived from the bytes on
        # disk, and the bytes just changed.
        Manifest.load(ROOT).write_lock!
        puts "corelib: re-locked #{Manifest::LOCK}"
        0
      end

      # A corelib file that reaches for a primitive is not vendorable: the
      # `Primitive.`/`__builtin`/`cexpr!` protocol is a C-level escape hatch
      # zeo has no answer for, and a file carrying one would compile and then
      # fail at the call. Refusing here is what keeps `corelib/` honest.
      def reject_primitives(rel, bytes)
        found = %w[Primitive. __builtin cexpr! cstmt! attr!].select { |m| bytes.include?(m) }
        return if found.empty?

        raise Error, "#{rel} uses #{found.join(", ")} -- the primitive protocol is not implemented, " \
                     "so this file cannot be corelib (see upstream.rb)"
      end

      # Which of upstream's `BUILTIN_RB_SRCS` carry no primitive at all.
      def cmd_census
        checkout = Vendor.fetch_checkout(entry, entry.rev)
        mk = File.read(File.join(checkout, "common.mk"))
        list = mk[/^BUILTIN_RB_SRCS = \\\n(.*?)^\s*\$\(empty\)/m].to_s
        names = list.scan(%r{\$\(srcdir\)/(\S+\.rb)}).flatten.sort
        vendored = entry.files.sort
        names.each do |rel|
          path = File.join(checkout, rel)
          next unless File.file?(path)

          bytes = File.binread(path)
          marks = %w[Primitive. __builtin cexpr! cstmt! attr!].sum { |m| bytes.scan(m).size }
          state = if vendored.include?(rel) then "vendored"
                  elsif marks.zero? then "ELIGIBLE"
                  else "needs #{marks} primitive(s)"
                  end
          puts format("%-24s %6d lines  %s", rel, bytes.count("\n"), state)
        end
        0
      end
    end
  end
end
