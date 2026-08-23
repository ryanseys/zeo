# frozen_string_literal: true

require "json"

module ZeoDev
  # The upstream manifest: which git-sourced trees zeo vendors, and at which
  # immutable commit.
  #
  # `upstream.rb` at the repo root is the SOURCE -- a ruby DSL, so a comment
  # sits beside the entry it explains and a conditional entry is expressible.
  # `upstream.lock` beside it is the DERIVED form: plain JSON, checked in, and
  # the only file anything other than this tool reads. Nothing outside
  # `tools/` should have to run ruby to learn what is pinned.
  #
  # Two groups. `gems` are vendored into the committed `gems/<name>/` (a stub
  # gemspec plus the verbatim `lib/`), which is what the compiler reads, so a
  # fresh clone builds offline. `gemtests` are fetched into the gitignored
  # `vendor/gemtests/<name>/` as WHOLE checkouts -- a `.gem` archive does not
  # ship `test/`, so those trees come from the repo and are fetched on demand.
  class Manifest
    SOURCE = "upstream.rb"
    LOCK = "upstream.lock"

    # One git-sourced tree.
    #
    # `rev` is the reproducible pin: a tag is resolved to a commit SHA once
    # and trusted from then on. `subdir` is the directory INSIDE the checkout
    # holding the tree, for a repo that ships more than one gem --
    # `rubygems/rubygems` carries bundler under `bundler/`.
    Entry = Struct.new(:name, :github, :tag, :rev, :subdir, :group, keyword_init: true) do
      def url = "https://github.com/#{github}"
      def version = tag.delete_prefix("v")
      def short_rev = rev ? rev[0, 12] : "(unresolved)"

      def to_h
        h = { "name" => name, "github" => github, "tag" => tag }
        h["rev"] = rev if rev
        h["subdir"] = subdir if subdir
        h
      end
    end

    # The DSL `upstream.rb` is evaluated against.
    class Dsl
      attr_reader :entries

      def initialize = @entries = []

      # `gem "fileutils", github: "ruby/fileutils", tag: "v1.8.0", rev: "..."`
      def gem(name, github:, tag:, rev: nil, subdir: nil)
        @entries << Entry.new(name: name, github: github, tag: tag, rev: rev,
                              subdir: subdir, group: :gems)
      end

      # A gem vendored only for its TEST tree.
      def gemtest(name, github:, tag:, rev: nil, subdir: nil)
        @entries << Entry.new(name: name, github: github, tag: tag, rev: rev,
                              subdir: subdir, group: :gemtests)
      end
    end

    attr_reader :entries

    def self.load(root = ROOT)
      path = File.join(root, SOURCE)
      raise Error, "#{SOURCE} is missing" unless File.exist?(path)

      dsl = Dsl.new
      dsl.instance_eval(File.read(path), path)
      new(dsl.entries.sort_by { |e| [e.group.to_s, e.name] }, root)
    end

    def initialize(entries, root = ROOT)
      @entries = entries
      @root = root
      dupes = entries.group_by { |e| [e.group, e.name] }.select { |_, v| v.size > 1 }.keys
      raise Error, "#{SOURCE} lists #{dupes.map(&:last).join(", ")} more than once" unless dupes.empty?
    end

    def group(name) = entries.select { |e| e.group == name }

    def find(name, group: :gems)
      entries.find { |e| e.group == group && e.name == name }
    end

    # The derived JSON. Sorted and two-space indented so its diff is
    # reviewable and its bytes do not depend on the machine.
    def lock_json
      "#{JSON.pretty_generate(
        "gems" => group(:gems).map(&:to_h),
        "gemtests" => group(:gemtests).map(&:to_h)
      )}\n"
    end

    def write_lock! = Tsv.write(File.join(@root, LOCK), lock_json)

    def lock_path = File.join(@root, LOCK)
  end
end
