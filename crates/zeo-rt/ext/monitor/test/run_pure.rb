# frozen_string_literal: true

# Runs the vendored upstream monitor suite (test_monitor.rb, verbatim from
# ruby/ruby at the ruby-headers.lock rev) against the RUBY HALF in ../lib,
# under a real ruby. ruby PRELOADS its own monitor.rb at boot, so a require
# can never reach zeo's -- `load` overlays it instead. Its `require
# "monitor.so"` resolves CRuby's C Monitor, the same
# enter/exit/wait_for_cond surface zeo's Rust half exports, so this
# differentially tests the delegation layer.
load File.expand_path("../lib/monitor.rb", __dir__)
require "bundler/setup"
require_relative "test_monitor"

# The one tool/lib/core_assertions.rb helper the suite uses: join every
# thread, surface any exception as a failure, answer the values.
class TestMonitor
  def assert_join_threads(threads, message = nil)
    values = []
    errors = []
    threads.each do |th|
      begin
        values << th.value
      rescue Exception => e
        errors << e
      end
    end
    unless errors.empty?
      msg = errors.map { |e| "#{e.class}: #{e.message}" }.join("\n")
      flunk(message ? "#{message}\n#{msg}" : msg)
    end
    values
  end
end
