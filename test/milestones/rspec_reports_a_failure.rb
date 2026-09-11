# MILESTONE: rspec reports a failing and a pending example. It renders each
# by extracting the source snippet for the line that raised, and
# `RSpec::Support::Source#ast` does that with `require "ripper"`.
#
# A suite whose examples all pass never reaches that path, which is what
# `an_rspec_suite_runs.rb` holds. This file is the other half.

require "stringio"
real_stdout = $stdout
$stdout = StringIO.new
at_exit do
  report = $stdout.string
  $stdout = real_stdout
  print report.gsub(/Finished in .*/, "Finished in <time>")
end

require "rspec/autorun"

RSpec.describe "arithmetic" do
  it "reports a failure" do
    expect(1).to eq(2)
  end

  it "is pending" do
    pending "not yet"
    raise "still broken"
  end
end
__END__
F*

Pending: (Failures listed here are expected and do not affect your suite's status)

  1) arithmetic is pending
     # not yet
     Failure/Error: raise "still broken"

     RuntimeError:
       still broken
     # ./milestones/rspec_reports_a_failure.rb:26:in 'block (2 levels) in <main>'

Failures:

  1) arithmetic reports a failure
     Failure/Error: expect(1).to eq(2)

       expected: 2
            got: 1

       (compared using ==)
     # ./milestones/rspec_reports_a_failure.rb:21:in 'block (2 levels) in <main>'

Finished in <time>
2 examples, 1 failure, 1 pending

Failed examples:

rspec ./milestones/rspec_reports_a_failure.rb:20 # arithmetic reports a failure

#@ exit 1
