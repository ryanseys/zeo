# frozen_string_literal: true

require "json"
require "stringio"
require "zlib"
require "rubygems/package"

module ZeoDev
  module Commands
    # Stage the two artifacts the published `zeo` crate ships but the repo
    # does not commit.
    #
    # - `crates/zeo/src/class_surface.pregen.rs` -- the builtin class-surface
    #   projection. NOT regenerated from scratch here: a `cargo build -p zeo`
    #   runs (a no-op when fresh) and the build script's own
    #   `$OUT_DIR/class_surface.rs` is copied, so the staged file is
    #   byte-identical to what the dev tree compiles against, by construction.
    # - `crates/zeo/gems.pregen.tar.gz` -- the bundled `gems/` tree, embedded
    #   into a registry-installed binary and extracted on first run. Built
    #   deterministically (sorted walk, zeroed mtimes) so `--check` compares
    #   content rather than compression accidents.
    #
    # `--check` verifies staged copies that EXIST still match a fresh
    # generation and exits nonzero on drift; absent artifacts are fine, since
    # the dev tree does not carry them. CI runs the check so a release can
    # never ship a stale staging.
    class StagePublish < Cli
      SURFACE = "crates/zeo/src/class_surface.pregen.rs"
      GEMS_TAR = "crates/zeo/gems.pregen.tar.gz"

      def self.summary = "stage the artifacts the published crate ships"
      def self.banner = "usage: zeo-dev stage-publish [--check]"

      def defaults = { check: false }

      def options(o)
        o.on("--check", "fail on drift, and write nothing") { opts[:check] = true }
      end

      def run
        expect_no_args!
        surface_dest = File.join(ROOT, SURFACE)
        gems_dest = File.join(ROOT, GEMS_TAR)
        surface = generated_class_surface
        gems_tar = build_gems_tar(File.join(ROOT, "gems"))

        return check(surface_dest, surface, gems_dest, gems_tar) if opts[:check]

        File.binwrite(surface_dest, surface)
        gz = gzip(gems_tar)
        File.binwrite(gems_dest, gz)
        puts "staged #{surface_dest} (#{surface.bytesize} bytes) and #{gems_dest} " \
             "(#{gz.bytesize} bytes -- #{gems_tar.bytesize} uncompressed)"
        0
      end

      private

      def check(surface_dest, surface, gems_dest, gems_tar)
        if File.file?(surface_dest) && File.binread(surface_dest) != surface
          warn "#{SURFACE} is STALE relative to the live projection -- re-run " \
               "`tools/zeo-dev stage-publish`"
          return 1
        end
        if File.file?(gems_dest)
          # Compare DECOMPRESSED tar bytes: the tar stream is deterministic
          # (sorted, zeroed mtimes); the gzip envelope need not be.
          staged = Zlib::GzipReader.open(gems_dest, &:read)
          if staged != gems_tar
            warn "#{GEMS_TAR} is STALE relative to gems/ -- re-run `tools/zeo-dev stage-publish`"
            return 1
          end
        end
        puts "stage-publish --check: staged artifacts match (or are absent)."
        0
      end

      # Build `-p zeo` (a no-op when fresh) and pull `class_surface.rs` out of
      # the build script's OUT_DIR, located through cargo's JSON message
      # stream -- never by globbing `target/`, where stale build dirs for
      # older fingerprints linger.
      def generated_class_surface
        res = Exec.run(%w[cargo build -p zeo --message-format=json-render-diagnostics],
                       chdir: ROOT, capture_stdout: true)
        raise Error, "cargo build -p zeo exited with #{res.code.inspect}" unless res.success?

        out_dir = nil
        res.stdout.each_line do |line|
          v = begin
            JSON.parse(line)
          rescue JSON::ParserError
            next
          end
          next unless v["reason"] == "build-script-executed"

          id = v["package_id"].to_s
          # A path package id ends `.../crates/zeo#<version>`; the separate
          # `#name@version` form appears when the dir name differs.
          out_dir = v["out_dir"] if id.include?("/crates/zeo#") || id.include?("#zeo@")
        end
        raise Error, "no build-script-executed message for zeo" if out_dir.nil?

        File.binread(File.join(out_dir, "class_surface.rs"))
      end

      # A deterministic tar of `gems/`: entries named relative to the dir, so
      # the extraction dir IS the gems dir; sorted; mtime 0; mode reduced to
      # 0644 or 0755. `.DS_Store` is skipped.
      def build_gems_tar(gems_dir)
        raise Error, "#{gems_dir} is not a directory" unless File.directory?(gems_dir)

        io = StringIO.new(+"".b)
        ::Gem::Package::TarWriter.new(io) do |tar|
          collect_files(gems_dir).each do |path|
            rel = path.delete_prefix("#{gems_dir}/")
            data = File.binread(path)
            mode = File.executable?(path) ? 0o755 : 0o644
            tar.add_file_simple(rel, mode, data.bytesize) { |f| f.write(data) }
          end
        end
        io.string
      end

      def collect_files(dir)
        Dir.glob("**/*", File::FNM_DOTMATCH, base: dir)
           .reject { |r| r == "." || r.end_with?("/.", "/..") || File.basename(r) == ".DS_Store" }
           .map { |r| File.join(dir, r) }
           .select { |p| File.file?(p) }
           .sort
      end

      # `Compression::best`, and mtime 0 so the envelope carries no clock.
      def gzip(bytes)
        io = StringIO.new(+"".b)
        gz = Zlib::GzipWriter.new(io, Zlib::BEST_COMPRESSION)
        gz.mtime = 0
        gz.write(bytes)
        gz.close
        io.string
      end
    end
  end
end
