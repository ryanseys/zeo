# frozen_string_literal: true

require "digest"
require "fileutils"
require "json"
require "tmpdir"

module ZeoDev
  module Commands
    # Assemble the relocatable distribution.
    #
    # One staged tree feeds every install channel -- the GitHub Release
    # tarball, Homebrew, and the rubygems platform gems all carry exactly
    # this:
    #
    #   zeo-<version>-<triple>/
    #     bin/zeo                          # dist-profile build
    #     share/zeo/
    #       dist-manifest.json             # {schema, version, target, vendored}
    #       gems/                          # the repo's gems/, verbatim
    #       runtime/                       # a REAL mini-workspace
    #       ...
    #     share/doc/zeo/                   # README + licenses
    #
    # The runtime workspace's root manifest is REWRITTEN from the live root
    # `Cargo.toml` -- members pruned to the four runtime crates, the `zeo`
    # workspace-dependency entry dropped, everything else carried verbatim --
    # so it cannot drift from what the dev tree builds.
    #
    # KNOWN BROKEN, and ported as-is rather than fixed here. The staged tree
    # never carries `libzeo.a`, so an installed `zeo -o` cannot find an
    # archive at all; the smoke test passes anyway because it uses `zeo -e`,
    # which takes the JIT path. Fixing that, and dropping the mini-workspace
    # and vendor tree whose only consumer was deleted, is the distribution
    # phase's first task.
    class Dist < Cli
      RUNTIME_CRATES = %w[zeo-rt zeo-abi zeo-dsl zeo-macros].freeze

      def self.summary = "assemble the relocatable distribution"

      def self.banner
        "usage: zeo-dev dist [--target <triple>] [--no-vendor] [--no-smoke] " \
          "[--stage-only] [-o <dir>]"
      end

      def defaults = { target: nil, vendor: true, smoke: true, stage_only: false, out: nil }

      def options(o)
        o.on("--target TRIPLE", "build for TRIPLE instead of the host") { |v| opts[:target] = v }
        o.on("--no-vendor", "skip the crates.io vendor tree") { opts[:vendor] = false }
        o.on("--no-smoke", "skip the smoke test") { opts[:smoke] = false }
        o.on("--stage-only", "stage and check coherence; build no binary") { opts[:stage_only] = true }
        o.on("-o DIR", "stage into DIR instead of target/dist") { |v| opts[:out] = v }
      end

      def run
        expect_no_args!
        version = workspace_version
        triple = opts[:target] || host_triple
        dist_name = "zeo-#{version}-#{triple}"
        out_dir = opts[:out] || File.join(ROOT, "target", "dist")
        stage = File.join(out_dir, dist_name)
        refuse_to_delete_our_own_cwd!(stage)
        FileUtils.rm_rf(stage)
        FileUtils.mkdir_p(stage)
        payload = File.join(stage, "share", "zeo")

        copy_tree(File.join(ROOT, "gems"), File.join(payload, "gems"))
        runtime = stage_runtime(payload, version)
        File.write(File.join(payload, "dist-manifest.json"), <<~JSON)
          {
            "schema": 1,
            "version": "#{version}",
            "target": "#{triple}",
            "vendored": #{opts[:vendor]}
          }
        JSON
        stage_docs(stage)

        if opts[:stage_only]
          # Coherence gate: the staged workspace must resolve with exactly its
          # shipped lock (and vendor tree, when present) -- what an installed
          # zeo's `--locked --offline` build will demand.
          check = %w[metadata --format-version 1 --locked]
          check << "--offline" if opts[:vendor]
          cargo!(runtime, check, quiet: true)
          puts "dist: staged #{stage} (stage-only, coherence OK)"
          return 0
        end

        stage_binary(stage)
        smoke_test(stage) if opts[:smoke]
        tarball(out_dir, dist_name)
        0
      end

      private

      def workspace_version
        line = File.readlines(File.join(ROOT, "Cargo.toml")).find { |l| l.start_with?("version = ") }
        raise Error, "the root manifest names no [workspace.package] version" if line.nil?

        line.split("=", 2).last.strip.delete('"')
      end

      def host_triple
        out = `rustc -vV 2>/dev/null`
        line = out.lines.find { |l| l.start_with?("host: ") }
        raise Error, "rustc -vV printed no host line" if line.nil?

        line.delete_prefix("host: ").strip
      end

      def stage_runtime(payload, _version)
        runtime = File.join(payload, "runtime")
        RUNTIME_CRATES.each do |name|
          copy_tree(File.join(ROOT, "crates", name), File.join(runtime, "crates", name),
                    skip: %w[target])
        end
        File.write(File.join(runtime, "Cargo.toml"), runtime_manifest)
        # The lock: seed with the root's (the versions the dev tree tested),
        # then let cargo prune the compiler-only entries.
        FileUtils.cp(File.join(ROOT, "Cargo.lock"), File.join(runtime, "Cargo.lock"))
        cargo!(runtime, %w[generate-lockfile])
        if opts[:vendor]
          cargo!(runtime, %w[vendor --locked vendor])
          FileUtils.mkdir_p(File.join(runtime, ".cargo"))
          File.write(File.join(runtime, ".cargo", "config.toml"), <<~TOML)
            [source.crates-io]
            replace-with = "vendored-sources"

            [source.vendored-sources]
            directory = "vendor"
          TOML
        end
        runtime
      end

      # The payload's runtime-workspace manifest, rewritten from the LIVE root
      # manifest so the two cannot drift: members pruned to the runtime
      # crates, the `zeo` workspace-dependency entry (whose path does not
      # exist in the payload) dropped, every other table carried verbatim.
      #
      # Line-based, because the schema is fixed and this repo owns it. The
      # two edits are a members array written on its own lines and one
      # dependency entry on one line.
      def runtime_manifest
        src = File.read(File.join(ROOT, "Cargo.toml"))
        members = RUNTIME_CRATES.map { |n| %(    "crates/#{n}",) }.join("\n")
        out = src.sub(/^members = \[\n.*?^\]\n/m, "members = [\n#{members}\n]\n")
        raise Error, "the root manifest's [workspace] members array did not match" if out == src

        # The comment block above the entry goes with it. It explains the
        # version lock on the `zeo` dependency, and a payload that does not
        # carry that dependency should not carry its rationale either.
        without_zeo = out.sub(/(?:^#[^\n]*\n)*^zeo = \{[^\n]*\}\n/, "")
        raise Error, "the root manifest has no `zeo = { .. }` workspace dependency" if without_zeo == out

        "# @generated by `tools/zeo-dev dist` from the repo's root Cargo.toml.\n#{without_zeo}"
      end

      def stage_docs(stage)
        doc = File.join(stage, "share", "doc", "zeo")
        FileUtils.mkdir_p(doc)
        %w[README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md].each do |name|
          src = File.join(ROOT, name)
          FileUtils.cp(src, File.join(doc, name)) if File.file?(src)
        end
      end

      # The `dist` profile: single-codegen-unit plus thin LTO, the
      # maximum-optimization shape the everyday `release` profile gives up for
      # build parallelism. Shipped artifacts pay it once per release.
      def stage_binary(stage)
        build = %w[build --profile dist -p zeo]
        build.push("--target", opts[:target]) if opts[:target]
        cargo!(ROOT, build)
        built = if opts[:target]
                  File.join(ROOT, "target", opts[:target], "dist", "zeo")
                else
                  File.join(ROOT, "target", "dist", "zeo")
                end
        FileUtils.mkdir_p(File.join(stage, "bin"))
        FileUtils.cp(built, File.join(stage, "bin", "zeo"))
      end

      # The staged tree must work with no help from the environment: a temp
      # cache, no ZEO_HOME, no ambient CARGO_TARGET_DIR.
      def smoke_test(stage)
        cache = File.join(Dir.tmpdir, "zeo-dist-smoke-#{Process.pid}")
        FileUtils.rm_rf(cache)
        puts "dist: smoke test (cold runtime build -- takes a few minutes)..."
        res = Exec.run([File.join(stage, "bin", "zeo"), "-e",
                        %(require "json"; puts JSON.generate({smoke: "ok"}))],
                       # An explicit cwd, so the test never depends on where
                       # dist was launched from -- and never inherits a
                       # directory staging deletes.
                       chdir: Dir.tmpdir,
                       env: { "ZEO_CACHE_DIR" => cache, "ZEO_HOME" => nil,
                              "CARGO_TARGET_DIR" => nil },
                       capture_stdout: true)
        ok = res.success? && res.stdout.include?(%({"smoke":"ok"}))
        FileUtils.rm_rf(cache)
        return puts("dist: smoke test passed") if ok

        raise Error, "smoke test FAILED (exit #{res.code.inspect}):\n" \
                     "stdout: #{res.stdout}\nstderr: #{res.stderr}"
      end

      def tarball(out_dir, dist_name)
        path = File.join(out_dir, "#{dist_name}.tar.gz")
        ok = system("tar", "-czf", path, "-C", out_dir, dist_name)
        raise Error, "tar failed" unless ok

        digest = Digest::SHA256.file(path).hexdigest
        File.write("#{path}.sha256", "#{digest}  #{dist_name}.tar.gz\n")
        puts "dist: #{path} (#{digest})"
      end

      # Staging wipes and recreates its output directory. If this process is
      # RUNNING inside that directory, the wipe pulls the working directory
      # out from under it and every later `chdir` fails with a baffling
      # "could not locate working directory" -- from cargo, from the staged
      # binary, from anything downstream.
      def refuse_to_delete_our_own_cwd!(stage)
        cwd = Dir.pwd
        abs = File.expand_path(stage)
        return unless cwd == abs || cwd.start_with?("#{abs}/")

        raise Error, "refusing to stage into #{abs} -- the current directory (#{cwd}) is inside " \
                     "it, and staging starts by deleting that tree. Run dist from elsewhere " \
                     "(the repo root is the usual choice)."
      end

      def cargo!(dir, args, quiet: false)
        res = Exec.run(["cargo", *args], chdir: dir, capture_stdout: quiet)
        # Not quiet: cargo's own progress belongs on the terminal. Quiet
        # swallows stdout because `metadata` dumps megabytes.
        raise Error, "cargo #{args.join(" ")} exited with #{res.code.inspect}" unless res.success?
      end

      def copy_tree(src, dst, skip: [])
        FileUtils.mkdir_p(dst)
        Dir.children(src).sort.each do |base|
          next if base == ".DS_Store" || skip.include?(base)

          from = File.join(src, base)
          to = File.join(dst, base)
          File.directory?(from) ? copy_tree(from, to, skip: skip) : FileUtils.cp(from, to)
        end
      end
    end
  end
end
