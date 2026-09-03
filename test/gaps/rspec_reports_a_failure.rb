# XFAIL: rspec renders a failing or pending example by extracting the source
# snippet for the line that raised, and `RSpec::Support::Source#ast` does that
# with `require "ripper"`.
#
# zeo declines ripper on purpose: it exposes the reduction event stream of
# CRuby's parse.y, and zeo's front end embeds prism -- a different parser with
# a different event model, so there is nothing to bind. rspec asks for it
# unconditionally on CRuby, deciding from RUBY_VERSION rather than probing, so
# the LoadError escapes through the formatter and the run dies after the
# progress dots.
#
# A suite whose examples all pass never reaches that path, which is what
# `../an_rspec_suite_runs.rb` holds. This file is the other half: the day zeo
# can answer `require "ripper"`, or rspec stops needing it, this starts
# matching and the suite says so.

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
     # ./milestones/pending/rspec_reports_a_failure.rb:35:in 'block (2 levels) in <main>'

Failures:

  1) arithmetic reports a failure
     Failure/Error: expect(1).to eq(2)

       expected: 2
            got: 1

       (compared using ==)
     # ./milestones/pending/rspec_reports_a_failure.rb:30:in 'block (2 levels) in <main>'

Finished in <time>
2 examples, 1 failure, 1 pending

Failed examples:

rspec ./milestones/pending/rspec_reports_a_failure.rb:29 # arithmetic reports a failure

#@ exit 1
