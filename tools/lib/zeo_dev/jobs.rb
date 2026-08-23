# frozen_string_literal: true

require "etc"

module ZeoDev
  # How many whole `zeo` compiles a sweep may run at once, and what each one
  # may hold.
  #
  # The limit is MEMORY, not cores. One gem-scale compile has been measured at
  # 9.8 GB resident; twelve at once -- one per core, which is what the sweeps
  # used to do -- exhausted a 16 GB machine's memory and swap and panicked the
  # kernel with a watchdog timeout. A hand-tuned `4` is the right number for
  # THAT machine and says nothing on any other.
  #
  # So the rule is a budget rather than a count. A fixed fraction of RAM is
  # the whole sweep's to spend, each job gets an equal share, and the share is
  # handed to the child as `ZEO_MEMORY_LIMIT` so the child enforces it on
  # itself. `--jobs` then trades job count against per-job headroom instead of
  # multiplying an unbounded number.
  #
  # On a 16 GB / 12-core machine this derives 4 jobs -- the number the
  # hand-tuned constant landed on, which is the calibration to check against.
  class Jobs
    # The share of RAM a sweep may spend across all its children at once. The
    # rest is the OS, the editor, this process, and the slack that keeps a
    # burst off the compressor.
    BUDGET_NUMERATOR = 3
    BUDGET_DENOMINATOR = 5

    # What one front-end compile is expected to want. Not a hard figure -- it
    # is the divisor that turns the budget into a job count.
    PER_JOB_TARGET = 2 * 1024 * 1024 * 1024

    # The smallest per-job ceiling worth handing out. Below this a compile
    # fails on its own startup.
    PER_JOB_FLOOR = 512 * 1024 * 1024

    # The fallback when the machine's RAM cannot be read.
    FALLBACK_JOBS = 4

    attr_reader :jobs, :per_job_bytes

    def initialize(requested = nil)
      cores = Etc.nprocessors
      total = self.class.physical_memory
      if total.nil?
        @jobs = [requested || FALLBACK_JOBS, 1].max
        @per_job_bytes = nil
        return
      end
      budget = total / BUDGET_DENOMINATOR * BUDGET_NUMERATOR
      derived = (budget / PER_JOB_TARGET).clamp(1, cores)
      @jobs = [requested || derived, 1].max
      per_job = budget / @jobs
      if per_job < PER_JOB_FLOOR
        warn "zeo-dev: #{@jobs} jobs leaves #{gib(per_job)} per compile, under the " \
             "#{gib(PER_JOB_FLOOR)} floor -- the machine's #{gib(total)} may not hold them all"
      end
      @per_job_bytes = [per_job, PER_JOB_FLOOR].max
    end

    # The child's share, as an environment hash to merge. A compile that
    # outruns it exits 12 instead of taking the machine down.
    def env = per_job_bytes ? { "ZEO_MEMORY_LIMIT" => per_job_bytes.to_s } : {}

    def describe
      per_job_bytes ? "#{jobs} job(s), #{gib(per_job_bytes)} each" : "#{jobs} job(s), unbounded"
    end

    # Runs `items` through `block` at `jobs` width, answering the results in
    # the INPUT order -- a sweep's ledger must not depend on which child
    # finished first.
    def each_parallel(items)
      results = Array.new(items.size)
      queue = Thread::Queue.new
      items.each_with_index { |item, i| queue << [item, i] }
      jobs.times { queue << nil }
      workers = Array.new(jobs) do
        Thread.new do
          while (pair = queue.pop)
            item, i = pair
            results[i] = yield(item)
          end
        end
      end
      workers.each(&:join)
      results
    end

    # The machine's RAM. `zeo::memguard::physical_memory`'s two reads, in
    # ruby: macOS answers `sysctl`, linux answers `/proc/meminfo`.
    def self.physical_memory
      if File.readable?("/proc/meminfo")
        line = File.foreach("/proc/meminfo").find { |l| l.start_with?("MemTotal:") }
        kb = line&.split&.at(1)
        return kb.to_i * 1024 if kb
      end
      out = `sysctl -n hw.memsize 2>/dev/null`
      n = out.to_i
      n.positive? ? n : nil
    rescue StandardError
      nil
    end

    private

    def gib(bytes) = format("%.1f GiB", bytes / (1024.0 * 1024.0 * 1024.0))
  end
end
