# THE parallel-default headline: a compute-only spinner in one thread
# cannot starve a sibling, because both run on independent OS threads. The
# worker computes its value concurrently while the spinner loops; the
# program only finishes once the spinner is killed.

Thread.report_on_exception = false
spinner = Thread.new { loop { } }
worker = Thread.new { 21 + 21 }
p worker.value
spinner.kill
spinner.join
p spinner.alive?
__END__
42
false
