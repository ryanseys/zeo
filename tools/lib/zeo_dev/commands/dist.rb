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
    #       dist-manifest.json             # {schema, version, target}
    #       gems/                          # the repo's gems/, verbatim
    #       lib/<triple>/libzeo.a          # what `zeo -o` links against
    #     share/doc/zeo/                   # README + licenses
    #
    # `libzeo.a` is the payload: `zeo -o` links a compiled program against it,
    # so a tree without it can run programs (the JIT path) and compile none.
    # It is keyed by triple because a payload may one day carry two -- see the
    # mac<->mac cross note in the distribution plan.
    class Dist < Cli

      def self.summary = "assemble the relocatable distribution"

      def self.banner
        "usage: zeo-dev dist [--target <triple>] [--no-smoke] " \
          "[--stage-only] [-o <dir>]"
      end

      def defaults = { target: nil, smoke: true, stage_only: false, out: nil }

      def options(o)
        o.on("--target TRIPLE", "build for TRIPLE instead of the host") { |v| opts[:target] = v }
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
        File.write(File.join(payload, "dist-manifest.json"), <<~JSON)
          {
            "schema": 1,
            "version": "#{version}",
            "target": "#{triple}"
          }
        JSON
        stage_docs(stage)

        if opts[:stage_only]
          puts "dist: staged #{stage} (stage-only, no binary and no archive)"
          return 0
        end

        stage_binary(stage, payload, triple)
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
      # One cargo invocation builds both halves: `zeo`'s crate-type is
      # `["rlib", "staticlib"]`, so the binary and `libzeo.a` come out of the
      # same profile directory and cannot be from different sources.
      def stage_binary(stage, payload, triple)
        build = %w[build --profile dist -p zeo]
        build.push("--target", opts[:target]) if opts[:target]
        cargo!(ROOT, build)
        built = if opts[:target]
                  File.join(ROOT, "target", opts[:target], "dist")
                else
                  File.join(ROOT, "target", "dist")
                end
        FileUtils.mkdir_p(File.join(stage, "bin"))
        FileUtils.cp(File.join(built, "zeo"), File.join(stage, "bin", "zeo"))
        archive = File.join(built, "libzeo.a")
        raise Error, "cargo built no #{archive} -- `zeo -o` cannot link without it" unless File.file?(archive)

        lib = File.join(payload, "lib", triple)
        FileUtils.mkdir_p(lib)
        FileUtils.cp(archive, File.join(lib, "libzeo.a"))
      end

      # The staged tree must work with no help from the environment: a temp
      # cache, no ZEO_HOME, no ambient CARGO_TARGET_DIR.
      #
      # It COMPILES and runs, rather than `zeo -e`. `-e` takes the JIT path,
      # which needs no archive, so it passed for as long as the staged tree
      # carried no `libzeo.a` at all. Only `-o` proves the payload.
      def smoke_test(stage)
        Dir.mktmpdir("zeo-dist-smoke") do |work|
          cache = File.join(work, "cache")
          src = File.join(work, "smoke.rb")
          bin = File.join(work, "smoke")
          File.write(src, %(require "json"\nputs JSON.generate({smoke: "ok"})\n))
          puts "dist: smoke test (compile and run)..."
          env = { "ZEO_CACHE_DIR" => cache, "ZEO_HOME" => nil, "CARGO_TARGET_DIR" => nil }
          zeo = File.join(stage, "bin", "zeo")
          # An explicit cwd, so the test never depends on where dist was
          # launched from -- and never inherits a directory staging deletes.
          build = Exec.run([zeo, "-o", bin, src], chdir: work, env: env,
                                                  capture_stdout: true)
          raise Error, smoke_failure("compile", build) unless build.success?

          res = Exec.run([bin], chdir: work, env: env, capture_stdout: true)
          raise Error, smoke_failure("run", res) unless res.success?
          unless res.stdout.include?(%({"smoke":"ok"}))
            raise Error, smoke_failure("output", res)
          end

          puts "dist: smoke test passed"
        end
      end

      def smoke_failure(stage, res)
        "smoke test FAILED at #{stage} (exit #{res.code.inspect}):\n" \
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
