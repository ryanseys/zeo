# frozen_string_literal: true

require "open3"

module ZeoDev
  # Running a child with a wall-clock deadline and bounded capture.
  #
  # A reader thread drains each pipe. Draining is not optional: a child that
  # fills a pipe blocks forever against a parent that never reads, and the
  # output of one gem-scale compile is measured in megabytes.
  #
  # stdout is drained and DISCARDED by default. Keeping it was measured at
  # near a gigabyte for one gem, and with several children in flight the
  # sweep's own memory was in the same class as the compiles it was measuring.
  # A caller that wants program output asks the child to write a file
  # (`zeo --emit-clif`), which is cheaper and the only shape that works at gem
  # scale. `capture_stdout: true` keeps it, bounded, for the callers that
  # genuinely need a few kilobytes.
  #
  # stderr IS kept, bounded at MAX_CAPTURE -- every caller classifies a
  # failure from it, and a diagnostic past 64 MiB is already past being read.
  module Exec
    MAX_CAPTURE = 64 << 20

    # `status` is nil when the child was killed on the deadline.
    Result = Struct.new(:stdout, :stderr, :status, :timed_out, keyword_init: true) do
      def success? = !status.nil? && status.success?
      def code = status&.exitstatus
    end

    module_function

    # `cmd` is an argv array. `env` is merged into the child's environment.
    def run(cmd, env: {}, stdin: nil, timeout: nil, chdir: nil, capture_stdout: false)
      spawn_opts = {}
      spawn_opts[:chdir] = chdir if chdir
      out = err = nil
      timed_out = false
      status = nil

      Open3.popen3(env.transform_keys(&:to_s), *cmd, **spawn_opts) do |i, o, e, t|
        writer = Thread.new do
          # The child may exit without reading; a broken pipe is fine.
          begin
            i.write(stdin) if stdin
          rescue Errno::EPIPE, IOError
            nil
          ensure
            begin
              i.close
            rescue IOError
              nil
            end
          end
        end
        reader_out = drain(o, capture_stdout ? MAX_CAPTURE : 0)
        reader_err = drain(e, MAX_CAPTURE)

        if timeout && !t.join(timeout)
          timed_out = true
          kill(t.pid)
        end
        status = t.value
        writer.join
        out = reader_out.value
        err = reader_err.value
      end

      Result.new(stdout: out, stderr: err, status: timed_out ? nil : status, timed_out: timed_out)
    end

    # Drains one pipe to EOF, keeping at most `keep` bytes.
    #
    # Draining continues PAST `keep` rather than stopping: a reader that stops
    # backpressures the child, which turns a big-output program into a bogus
    # timeout.
    def drain(io, keep)
      Thread.new do
        buf = +""
        buf.force_encoding(Encoding::BINARY)
        loop do
          chunk = begin
            io.readpartial(64 * 1024)
          rescue EOFError, IOError, Errno::EIO
            break
          end
          next if buf.bytesize >= keep

          room = keep - buf.bytesize
          buf << (chunk.bytesize <= room ? chunk : chunk.byteslice(0, room))
        end
        buf
      end
    end

    # TERM first, KILL if the child ignores it. A compile holding gigabytes
    # deserves the chance to unwind; nothing deserves to stay.
    def kill(pid)
      Process.kill("TERM", pid)
      20.times do
        return if Process.waitpid(pid, Process::WNOHANG)

        sleep 0.05
      end
      Process.kill("KILL", pid)
    rescue Errno::ESRCH, Errno::ECHILD
      nil
    end
  end
end
